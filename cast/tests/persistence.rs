use cast::{
    Result,
    adapters::prepare_query,
    models::{Budgets, CompanyDraft, Config, JobDraft, Snapshot, VerificationResult},
    store::Store,
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::Path,
    process::{Command, Output},
};

fn command(state: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cast"));
    command
        .arg("--state-dir")
        .arg(state)
        .env_remove("THEIRSTACK_API_KEY")
        .env_remove("BRAVE_SEARCH_API_KEY");
    command
}

fn output_json(output: &Output) -> Result<Value> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

#[test]
fn uninitialized_read_does_not_create_state() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let state = directory.path().join("not-initialized");
    let output = command(&state).args(["status", "--json"]).output()?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not initialized"));
    assert!(!state.exists());
    Ok(())
}

#[test]
fn process_lock_blocks_mutation_while_reads_remain_available() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let state = directory.path().join("state");
    let store = Store::init(&state)?;
    let revision = store.snapshot()?.snapshot_revision;
    let lock = store.lock()?;
    let blocked = command(&state)
        .args(["source", "add", "https://employer.example/careers"])
        .output()?;
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("another Cast mutation"));

    let status = output_json(&command(&state).args(["status", "--json"]).output()?)?;
    let snapshot = output_json(&command(&state).args(["export", "--json"]).output()?)?;
    assert_eq!(status["snapshot_revision"], revision);
    assert_eq!(snapshot["snapshot_revision"], revision);
    assert!(
        snapshot["companies"]
            .as_array()
            .ok_or("snapshot should contain a companies array")?
            .is_empty()
    );
    assert_eq!(store.snapshot()?.snapshot_revision, revision);

    drop(lock);
    output_json(
        &command(&state)
            .args(["source", "add", "https://employer.example/careers"])
            .output()?,
    )?;
    assert_eq!(store.snapshot()?.companies.len(), 1);
    Ok(())
}

#[test]
fn export_is_private_atomic_and_accepts_a_relative_filename() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let state = directory.path().join("state");
    let store = Store::init(&state)?;
    store.add_manual_source("https://employer.example/careers", None)?;
    let revision = store.snapshot()?.snapshot_revision;
    let destination = directory.path().join("snapshot.json");
    fs::write(&destination, "old snapshot")?;
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o644))?;
    output_json(
        &command(&state)
            .current_dir(directory.path())
            .args(["export", "--output", "snapshot.json"])
            .output()?,
    )?;
    let exported: Snapshot = serde_json::from_slice(&fs::read(&destination)?)?;
    assert_eq!(exported.snapshot_revision, revision);
    assert_eq!(exported.companies.len(), 1);
    assert_eq!(
        fs::metadata(&destination)?.permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(store.snapshot()?.snapshot_revision, revision);
    assert!(
        fs::read_dir(directory.path())?
            .collect::<std::io::Result<Vec<_>>>()?
            .iter()
            .all(|entry| {
                !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".cast-export-")
            })
    );
    Ok(())
}

#[test]
fn export_cannot_replace_live_state_or_its_lock_through_aliases() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let state = directory.path().join("state");
    let store = Store::init(&state)?;
    let lock = store.lock()?;
    let lock_inode = fs::metadata(state.join("cast.lock"))?.ino();
    let alias = directory.path().join("state-alias");
    symlink(&state, &alias)?;
    for name in [
        "cast.sqlite3",
        "cast.sqlite3-wal",
        "cast.sqlite3-shm",
        "cast.sqlite3-journal",
        "cast.lock",
    ] {
        for parent in [&state, &alias] {
            let error = store
                .atomic_export(&parent.join(name))
                .err()
                .ok_or("operation should fail")?;
            assert!(error.to_string().contains("protected Cast state"));
        }
    }
    let lock_alias = directory.path().join("lock-alias");
    symlink(state.join("cast.lock"), &lock_alias)?;
    assert!(store.atomic_export(&lock_alias).is_err());
    assert_eq!(fs::metadata(state.join("cast.lock"))?.ino(), lock_inode);
    assert!(Store::open(&state)?.lock().is_err());
    assert!(Store::open(&state)?.snapshot()?.companies.is_empty());
    drop(lock);
    assert!(Store::open(&state)?.lock().is_ok());
    Ok(())
}

#[test]
fn unsettled_usage_survives_restart_and_settled_usage_releases_only_unused_units() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::init(directory.path())?;
    let budgets = Budgets {
        theirstack_total_credits: 5,
        theirstack_daily_credits: 5,
        http_per_run: 10,
        ..Budgets::default()
    };
    let run = store.start_run()?;
    let settled = store.reserve_request(&run, "theirstack", 3, &budgets)?;
    store.settle_request(&settled, Some(1))?;
    store.reserve_request(&run, "theirstack", 3, &budgets)?;
    drop(store);

    let store = Store::open(directory.path())?;
    let run = store.start_run()?;
    assert_eq!(store.status()?["usage"]["theirstack"]["total_units"], 4);
    assert!(
        store
            .reserve_request(&run, "theirstack", 2, &budgets)
            .is_err()
    );
    let request = store.reserve_request(&run, "theirstack", 1, &budgets)?;
    store.settle_request(&request, None)?;
    assert!(
        store
            .reserve_request(&run, "theirstack", 1, &budgets)
            .is_err()
    );
    let status = store.status()?;
    assert_eq!(status["usage"]["theirstack"]["total_units"], 5);
    assert_eq!(status["http_requests_today"], 3);
    Ok(())
}

#[test]
fn frozen_query_window_resumes_and_reconfiguration_discards_old_coverage() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::init(directory.path())?;
    let mut config = Config::default();
    let query = config
        .queries
        .iter()
        .find(|query| query.provider == "theirstack")
        .ok_or("default configuration should include TheirStack")?;
    let mut coverage = store.coverage(query)?;
    coverage.high_watermark = Some("2026-01-03T00:00:00Z".into());
    let prepared = prepare_query(query, coverage.high_watermark.as_deref())?;
    coverage.cursor = prepared.cursor;
    coverage
        .cursor
        .as_mut()
        .ok_or("prepared query should have a frozen cursor")?["page"] = json!(2);
    coverage.status = "budget_exhausted".into();
    coverage.next_due_at = i64::MAX;
    store.save_coverage(&coverage)?;
    drop(store);

    let store = Store::open(directory.path())?;
    let restored = store.coverage(query)?;
    assert_eq!(restored.cursor, coverage.cursor);
    assert_eq!(restored.high_watermark, coverage.high_watermark);
    let mut resumed = query.clone();
    resumed.cursor = restored.cursor;
    assert_eq!(
        prepare_query(&resumed, Some("2026-02-01T00:00:00Z"))?.cursor,
        coverage.cursor
    );

    let query = config
        .queries
        .iter_mut()
        .find(|query| query.provider == "theirstack")
        .ok_or("configuration should retain TheirStack query")?;
    query.terms = vec!["database engineer".into()];
    let changed = query.clone();
    store.set_config(&config)?;
    let reset = store.coverage(&changed)?;
    assert_eq!(reset.status, "never_run");
    assert_eq!(reset.next_due_at, 0);
    assert!(reset.cursor.is_none());
    assert!(reset.high_watermark.is_none());
    assert!(store.snapshot()?.coverage.is_empty());
    Ok(())
}

fn job(number: u8) -> JobDraft {
    JobDraft {
        source_key: format!("fixture:{number}"),
        title: format!("Engineer {number}"),
        url: format!("https://employer.example/jobs/{number}"),
        is_listed: true,
        ..JobDraft::default()
    }
}

fn collected_jobs() -> Vec<JobDraft> {
    [
        "Software Engineer",
        "PLATFORM ENGINEER",
        "eNgInEeR",
        "Engineering Manager",
        "Developer",
        "Designer",
        "",
        "   ",
    ]
    .into_iter()
    .zip(1..)
    .map(|(title, number)| JobDraft {
        title: title.into(),
        description: Some("Work with engineers".into()),
        ..job(number)
    })
    .collect()
}

#[test]
fn discovery_filters_job_titles_before_storage_and_keeps_collection_progress() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::init(directory.path())?;
    let config = Config::default();
    let query = config
        .queries
        .iter()
        .find(|query| query.provider == "theirstack")
        .ok_or("default configuration should include TheirStack")?;
    let mut coverage = store.coverage(query)?;
    coverage.status = "partial".into();
    coverage.cursor = Some(json!({"page": 1}));
    let company = CompanyDraft {
        name: "Employer".into(),
        domain: Some("employer.example".into()),
        careers_urls: vec!["https://employer.example/careers".into()],
        jobs: collected_jobs(),
        ..CompanyDraft::default()
    };
    let run = store.start_run()?;
    let request = store.reserve_request(&run, "theirstack", 8, &config.budgets)?;
    store.settle_request(&request, Some(8))?;
    store.record_discovery(&[company], "discovery:fixture", &coverage)?;

    let excluded = CompanyDraft {
        name: "Another employer".into(),
        domain: Some("another.example".into()),
        jobs: vec![JobDraft {
            title: "Designer".into(),
            url: "not a URL".into(),
            ..job(9)
        }],
        ..CompanyDraft::default()
    };
    store.ingest_company(&excluded, "discovery:fixture")?;
    coverage.status = "complete".into();
    coverage.cursor = None;
    coverage.last_complete_at = Some(cast::now());
    store.record_discovery(&[excluded], "discovery:fixture", &coverage)?;
    drop(store);

    let store = Store::open(directory.path())?;
    let snapshot = store.snapshot()?;
    assert_eq!(snapshot.companies.len(), 2);
    assert_eq!(snapshot.source_health.len(), 1);
    assert_eq!(snapshot.jobs.len(), 4);
    for expected in collected_jobs().iter().take(4) {
        assert!(snapshot.jobs.iter().any(|job| {
            job.title == expected.title
                && job.source_key == expected.source_key
                && job.availability == "unknown"
        }));
    }
    assert_eq!(store.coverage(query)?.status, "complete");
    assert!(store.coverage(query)?.cursor.is_none());
    assert_eq!(
        store.coverage(query)?.last_complete_at,
        coverage.last_complete_at
    );
    assert_eq!(store.status()?["usage"]["theirstack"]["total_units"], 8);
    Ok(())
}

#[test]
fn careers_collection_filters_job_titles_before_storage() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::init(directory.path())?;
    let source = store.add_manual_source("https://employer.example/careers", None)?;
    store.record_verification(
        &source,
        &VerificationResult {
            jobs: collected_jobs(),
            complete: true,
            outcome: "complete".into(),
            ..VerificationResult::default()
        },
        86400,
    )?;
    drop(store);

    let snapshot = Store::open(directory.path())?.snapshot()?;
    assert_eq!(snapshot.jobs.len(), 4);
    for expected in collected_jobs().iter().take(4) {
        assert!(snapshot.jobs.iter().any(|job| {
            job.title == expected.title
                && job.source_key == expected.source_key
                && job.availability == "listed"
        }));
    }
    assert_eq!(snapshot.source_health[0].status, "complete");
    assert!(snapshot.source_health[0].last_success_at.is_some());
    Ok(())
}

#[test]
fn excluded_title_changes_preserve_jobs_and_scan_presence_across_restart() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::init(directory.path())?;
    let source = store.add_manual_source("https://employer.example/careers", None)?;
    store.record_verification(
        &source,
        &VerificationResult {
            jobs: vec![job(1), job(2)],
            complete: true,
            outcome: "complete".into(),
            ..VerificationResult::default()
        },
        86400,
    )?;
    let before = store.snapshot()?.jobs;
    store.record_verification(
        &store.source(&source.id)?,
        &VerificationResult {
            jobs: vec![
                JobDraft {
                    title: "Developer".into(),
                    url: "https://employer.example/jobs/new-url".into(),
                    ..job(1)
                },
                JobDraft {
                    title: "Designer".into(),
                    source_key: "fixture:new-key".into(),
                    url: "https://employer.example/jobs/2?utm_source=fixture".into(),
                    ..job(2)
                },
            ],
            next_cursor: Some(json!({"offset": 2})),
            outcome: "partial".into(),
            ..VerificationResult::default()
        },
        86400,
    )?;
    drop(store);

    let store = Store::open(directory.path())?;
    store.record_verification(
        &store.source(&source.id)?,
        &VerificationResult {
            jobs: vec![JobDraft {
                title: "Product Manager".into(),
                ..job(3)
            }],
            complete: true,
            outcome: "complete".into(),
            ..VerificationResult::default()
        },
        86400,
    )?;
    assert_eq!(
        serde_json::to_value(store.snapshot()?.jobs)?,
        serde_json::to_value(before)?
    );
    assert_eq!(store.source(&source.id)?.status, "complete");
    assert!(store.source(&source.id)?.cursor.is_none());
    Ok(())
}

#[test]
fn completed_pagination_remembers_jobs_seen_before_restart() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let store = Store::init(directory.path())?;
    let source = store.add_manual_source("https://employer.example/careers", None)?;
    let initial = VerificationResult {
        jobs: vec![job(1)],
        complete: true,
        outcome: "complete".into(),
        ..VerificationResult::default()
    };
    store.record_verification(&source, &initial, 86400)?;
    let source = store.source(&source.id)?;
    let first_page = VerificationResult {
        jobs: vec![job(1)],
        next_cursor: Some(json!({"offset": 1})),
        outcome: "partial".into(),
        ..VerificationResult::default()
    };
    store.record_verification(&source, &first_page, 86400)?;
    drop(store);

    let store = Store::open(directory.path())?;
    let source = store.source(&source.id)?;
    assert_eq!(source.cursor, first_page.next_cursor);
    store.record_verification(
        &source,
        &VerificationResult {
            jobs: vec![job(2)],
            ..initial
        },
        86400,
    )?;
    let snapshot = store.snapshot()?;
    assert_eq!(snapshot.jobs.len(), 2);
    assert!(
        snapshot
            .jobs
            .iter()
            .all(|job| { job.availability == "listed" && job.missing_complete_snapshots == 0 })
    );
    assert!(snapshot.source_health[0].cursor.is_none());
    assert!(snapshot.source_health[0].last_success_at.is_some());
    Ok(())
}

#[test]
fn compact_source_pages_preserve_full_export_and_reveal_more_results() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let state = directory.path().join("state");
    let store = Store::init(&state)?;
    for index in 0..21 {
        store.add_manual_source(&format!("https://employer-{index}.example/careers"), None)?;
    }
    let page = output_json(
        &command(&state)
            .args(["sources", "list", "--json"])
            .output()?,
    )?;
    assert_eq!(page["schema_version"], 2);
    assert_eq!(page["items"].as_array().map(Vec::len), Some(20));
    assert_eq!(page["has_more"], true);
    assert!(page["items"][0]["url"].is_string());
    let expanded = output_json(
        &command(&state)
            .args(["sources", "list", "--json", "--limit", "21"])
            .output()?,
    )?;
    assert_eq!(expanded["items"].as_array().map(Vec::len), Some(21));
    assert_eq!(expanded["has_more"], false);
    let full = output_json(&command(&state).args(["export", "--json"]).output()?)?;
    assert_eq!(full["schema_version"], 1);
    assert_eq!(full["source_health"].as_array().map(Vec::len), Some(21));
    let status = output_json(&command(&state).args(["status", "--json"]).output()?)?;
    assert_eq!(status["schema_version"], 2);
    assert!(status["budgets"].is_object());
    Ok(())
}
