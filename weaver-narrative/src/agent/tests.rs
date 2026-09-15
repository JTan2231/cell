use super::*;
use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::any,
};
use std::os::unix::fs::PermissionsExt as _;
use std::sync::{Arc, Mutex};

fn fixture() -> Result<(tempfile::TempDir, Store, Config, JobRequestV1)> {
    let temp = tempfile::tempdir()?;
    let store = Store::initialize(temp.path())?;
    let binary = temp.path().join("annals");
    let library = "0123456789abcdef0123456789abcdef";
    let token =
        |t| json!({"ok":true,"data":{"contract_version":2,"library_id":library,"watermark":t}});
    let page = json!({"ok":true,"data":{"contract_version":2,"library_id":library,"watermark":"end","request_cursor":"start","next_cursor":"item-1","events":[{
        "cursor":"item-1","event_id":"e1","document_id":"d1","source_name":"decision.md",
        "source_sha256":"2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        "accepted_at":"2026-09-10T08:00:00Z","document":"hello"}]}});
    let end = json!({"ok":true,"data":{"contract_version":2,"library_id":library,"watermark":"end","request_cursor":"item-1","next_cursor":"item-1","events":[]}});
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\ncase \"$5\" in\nstart) printf '%s\\n' '{}' ;;\nwatermark) printf '%s\\n' '{}' ;;\npage) case \"$9\" in\nstart) printf '%s\\n' '{}' ;;\nitem-1) printf '%s\\n' '{}' ;;\n*) exit 1 ;;\nesac ;;\n*) exit 1 ;;\nesac\n",
            token("start"),
            token("end"),
            page,
            end
        ),
    )?;
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))?;
    let config = Config {
        annals_binary: binary,
        annals_config: temp.path().join("config.toml"),
    };
    let request = request(
        "Cell helped me find a job. Write warmly for a friend.",
        None,
        temp.path(),
    )?;
    store.create(&request)?;
    Ok((temp, store, config, request))
}

#[allow(clippy::needless_pass_by_value)] // JSON fixtures are clearer as owned values.
fn call(request: &JobRequestV1, id: &str, tool: &str, args: Value) -> Result<ToolCallV1> {
    Ok(ToolCallV1 {
        version: 1,
        id: id.into(),
        job_id: request.id.clone(),
        attempt_id: "attempt-1".into(),
        request_sequence: 1,
        tool_name: tool.into(),
        arguments_schema_id: schema_id(tool).into(),
        arguments: to_raw_value(&args)?,
    })
}

#[test]
fn free_form_direction_and_revision_are_not_editorial_fields() -> Result<()> {
    let (_temp, store, _config, request) = fixture()?;
    let input: Value = serde_json::from_str(&request.prompt)?;
    assert_eq!(input.as_object().context("request input object")?.len(), 1);
    assert_eq!(
        input["direction"],
        "Cell helped me find a job. Write warmly for a friend."
    );
    assert_eq!(store.document(request.id.as_str())?.request, request);
    assert_eq!(request.invocation.workspace_access, WorkspaceAccess::None);
    assert!(!request.invocation.builtin_tools.local_execution);
    assert!(!request.invocation.builtin_tools.web_search);
    let revised = super::request(
        "Make this shorter",
        Some("Existing prose"),
        Path::new("/tmp"),
    )?;
    assert_eq!(
        serde_json::from_str::<Value>(&revised.prompt)?["document"],
        "Existing prose"
    );
    assert_ne!(revised.id, request.id);
    assert!(super::request(" ", None, Path::new("/tmp")).is_err());
    Ok(())
}

#[test]
fn reads_are_on_demand_with_short_pages_and_explicit_errors() -> Result<()> {
    let (_temp, _store, config, _request) = fixture()?;
    let first = read_decisions(&config, "{}")?;
    assert_eq!(first["events"][0]["document"], "hello");
    assert_eq!(first["has_more"], true);
    assert_eq!(
        read_decisions(&config, r#"{"after":"item-1","watermark":"end"}"#)?["has_more"],
        false
    );
    assert!(read_decisions(&config, r#"{"after":"bad","watermark":"end"}"#).is_err());
    assert!(read_decisions(&config, r#"{"after":"item-1"}"#).is_err());
    assert!(read_decisions(&config, r#"{"limit":201}"#).is_err());
    Ok(())
}

#[test]
fn output_and_reply_commit_together_and_conflicting_writes_fail() -> Result<()> {
    let (temp, store, config, request) = fixture()?;
    let first = call(
        &request,
        "c1",
        "submit_document",
        json!({"markdown":"# A story\n\nSome prose."}),
    )?;
    let accepted = prepare_reply(&store, &config, &request, &first)?;
    assert!(!accepted.reply.is_error);
    drop(store);
    let reopened = Store::open(temp.path(), false)?;
    assert_eq!(
        reopened.document(request.id.as_str())?.markdown.as_deref(),
        Some("# A story\n\nSome prose.")
    );
    let replay = prepare_reply(&reopened, &config, &request, &first)?;
    assert_eq!(
        serde_json::to_value(replay.reply)?,
        serde_json::to_value(accepted.reply)?
    );
    let changed = call(
        &request,
        "c1",
        "submit_document",
        json!({"markdown":"Changed"}),
    )?;
    assert!(prepare_reply(&reopened, &config, &request, &changed).is_err());
    reopened
        .connection
        .execute("UPDATE documents SET pending_reply=NULL", [])?;
    let second = call(
        &request,
        "c2",
        "submit_document",
        json!({"markdown":"Changed"}),
    )?;
    assert!(
        prepare_reply(&reopened, &config, &request, &second)?
            .reply
            .is_error
    );
    assert_eq!(
        reopened.document(request.id.as_str())?.markdown.as_deref(),
        Some("# A story\n\nSome prose.")
    );
    Ok(())
}

#[derive(Default)]
struct Mock {
    request: Option<JobRequestV1>,
    calls: Vec<ToolCallV1>,
    replies: std::collections::BTreeMap<String, Value>,
    submissions: usize,
    posts: usize,
    lose_reply: bool,
    terminal: &'static str,
    unhealthy: bool,
    queued: usize,
    started_at: Option<String>,
    cancellations: usize,
}

async fn respond(State(state): State<Arc<Mutex<Mock>>>, request: Request) -> Response {
    match respond_inner(state, request).await {
        Ok(response) => response,
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

#[allow(clippy::too_many_lines)] // Keep the small protocol fixture in one dispatcher.
async fn respond_inner(state: Arc<Mutex<Mock>>, request: Request) -> Result<Response> {
    let path = request.uri().path().to_owned();
    let method = request.method().clone();
    let bytes = to_bytes(request.into_body(), 4_194_304)
        .await
        .context("read mock request")?;
    let input: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let mut mock = state
        .lock()
        .map_err(|e| anyhow::anyhow!("mock state lock: {e}"))?;
    let ok = |value: Value| Ok(Json(value).into_response());
    if path == "/v1/health" {
        let capabilities = [
            HarnessCapability::ExactModel,
            HarnessCapability::ReasoningEffort,
            HarnessCapability::WorkspaceNone,
            HarnessCapability::BuiltinLocalExecution,
            HarnessCapability::BuiltinWebSearch,
            HarnessCapability::DynamicClientTools,
        ];
        return ok(
            json!({"version":1,"status":"ok","daemonVersion":"fixture","acceptingJobs":!mock.unhealthy,
            "checkedAt":"2026-09-14T00:00:00Z","supportedProtocolVersions":[1],"capabilities":capabilities,
            "harness":{"harness":"codex","harnessVersion":"fixture","adapterVersion":"fixture"},"harnessExecutable":"/fixture/codex",
            "authentication":{"codexHome":"/tmp/fixture","configured":true,"authenticated":true},
            "execution":{"maxActiveJobs":8,"activeJobs":8,"availableSlots":0}}),
        );
    }
    if path.starts_with("/v1/schemas") {
        return ok(input);
    }
    if path.starts_with("/v1/toolsets") {
        return ok(
            json!({"version":1,"toolset":input["toolset"],"digest":input["digest"],"definitionsSchemaId":input["definitionsSchemaId"],"registeredAt":"now"}),
        );
    }
    if path == "/v1/jobs" && method == axum::http::Method::POST {
        let request: JobRequestV1 = serde_json::from_value(input)?;
        if mock.request.as_ref().is_some_and(|old| *old != request) {
            return Ok(StatusCode::CONFLICT.into_response());
        }
        mock.submissions += 1;
        mock.request = Some(request.clone());
        return ok(
            json!({"version":1,"jobId":request.id,"state":"accepted","requestDigest":"fixture","logCursor":0}),
        );
    }
    let Some(request) = mock.request.clone() else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(json!({"version":1,"code":"not_found","message":"job absent"})),
        )
            .into_response());
    };
    if path.ends_with("/cancel") {
        mock.cancellations += 1;
        return ok(
            json!({"version":1,"jobId":request.id,"state":"running","cancellationRequested":true}),
        );
    }
    if path.ends_with("/result") {
        let id = input["callId"].as_str().context("tool call ID")?;
        if mock.replies.get(id).is_some_and(|old| *old != input) {
            return Ok(StatusCode::CONFLICT.into_response());
        }
        mock.replies.insert(id.to_owned(), input.clone());
        mock.posts += 1;
        if mock.lose_reply {
            mock.lose_reply = false;
            return Ok(StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
        let call = mock
            .calls
            .iter()
            .find(|c| c.id.as_str() == id)
            .context("known tool call")?;
        return ok(
            json!({"version":1,"call":call,"state":"answered","createdAt":"now","answeredAt":"now"}),
        );
    }
    if path.ends_with("/tool-calls") {
        let calls: Vec<_> = mock
            .calls
            .iter()
            .filter(|c| !mock.replies.contains_key(c.id.as_str()))
            .take(1)
            .map(|call| json!({"version":1,"call":call,"state":"pending","createdAt":"now"}))
            .collect();
        return ok(json!({"version":1,"jobId":request.id,"calls":calls,"nextSequence":1}));
    }
    let state = if mock.replies.len() == mock.calls.len() {
        mock.terminal
    } else if mock.queued > 0 {
        mock.queued -= 1;
        "accepted"
    } else {
        "waiting_on_requester"
    };
    ok(
        json!({"version":1,"summary":{"version":1,"id":request.id,"label":"test","requester":request.requester,
        "state":state,"requestDigest":"fixture","createdAt":"now","updatedAt":"now","currentAttemptId":"attempt-1"},
        "request":request,"attempts":[{"version":1,"id":"attempt-1","jobId":request.id,"ordinal":1,
        "harness":{"harness":"codex","harnessVersion":"fixture","adapterVersion":"fixture"},
        "state":if state == "accepted" {"pending"} else {"running"},"createdAt":"now",
        "startedAt":if state == "accepted" {None} else {Some(mock.started_at.clone().unwrap_or(OffsetDateTime::now_utc().format(&Rfc3339)?))} }]}),
    )
}

fn server(
    temp: &Path,
    state: Arc<Mutex<Mock>>,
) -> Result<(NucleusClient, tokio::task::JoinHandle<std::io::Result<()>>)> {
    let socket = temp.join("nucleus.sock");
    let listener = tokio::net::UnixListener::bind(&socket)?;
    let router = Router::new().fallback(any(respond)).with_state(state);
    let task = tokio::spawn(async move { axum::serve(listener, router).await });
    Ok((NucleusClient::new(socket)?, task))
}

#[tokio::test]
async fn interrupted_read_reply_replays_then_discards_source_bookkeeping() -> Result<()> {
    let (temp, store, config, request) = fixture()?;
    let state = Arc::new(Mutex::new(Mock {
        calls: vec![
            call(&request, "read", "read_decisions", json!({}))?,
            call(
                &request,
                "write",
                "submit_document",
                json!({"markdown":"The authored story."}),
            )?,
        ],
        lose_reply: true,
        terminal: "completed",
        queued: 2,
        ..Default::default()
    }));
    let (client, task) = server(temp.path(), Arc::clone(&state))?;
    let interrupted = run(&client, &store, &config, &request).await;
    assert!(interrupted.is_err());
    assert!(
        pending_reply(&store, request.id.as_str())?.is_some(),
        "{interrupted:?}"
    );
    drop(store);
    let store = Store::open(temp.path(), false)?;
    run(&client, &store, &config, &request).await?;
    assert_eq!(
        state
            .lock()
            .map_err(|e| anyhow::anyhow!("mock state lock: {e}"))?
            .submissions,
        1
    );
    assert_eq!(
        state
            .lock()
            .map_err(|e| anyhow::anyhow!("mock state lock: {e}"))?
            .posts,
        3
    );
    assert!(pending_reply(&store, request.id.as_str())?.is_none());
    assert_eq!(
        store.document(request.id.as_str())?.markdown.as_deref(),
        Some("The authored story.")
    );
    assert!(store.document(request.id.as_str())?.finished_at.is_some());
    task.abort();
    Ok(())
}

#[tokio::test]
async fn saved_document_survives_later_runtime_failure_and_no_new_attempt() -> Result<()> {
    let (temp, store, config, request) = fixture()?;
    let state = Arc::new(Mutex::new(Mock {
        calls: vec![call(
            &request,
            "write",
            "submit_document",
            json!({"markdown":"Saved prose."}),
        )?],
        terminal: "failed",
        ..Default::default()
    }));
    let (client, task) = server(temp.path(), Arc::clone(&state))?;
    let failed = run(&client, &store, &config, &request).await;
    assert!(failed.is_err());
    assert_eq!(
        store.document(request.id.as_str())?.markdown.as_deref(),
        Some("Saved prose."),
        "{failed:?}"
    );
    assert!(run(&client, &store, &config, &request).await.is_err());
    assert_eq!(
        state
            .lock()
            .map_err(|e| anyhow::anyhow!("mock state lock: {e}"))?
            .submissions,
        1
    );
    task.abort();
    Ok(())
}

#[tokio::test]
async fn lost_or_cancelled_jobs_without_documents_stay_failed() -> Result<()> {
    for terminal in ["failed", "cancelled", "completed"] {
        let (temp, store, config, request) = fixture()?;
        let state = Arc::new(Mutex::new(Mock {
            request: Some(request.clone()),
            terminal,
            ..Default::default()
        }));
        let (client, task) = server(temp.path(), Arc::clone(&state))?;
        assert!(run(&client, &store, &config, &request).await.is_err());
        assert!(store.document(request.id.as_str())?.markdown.is_none());
        assert!(store.document(request.id.as_str())?.error.is_some());
        assert_eq!(
            state
                .lock()
                .map_err(|e| anyhow::anyhow!("mock state lock: {e}"))?
                .submissions,
            0
        );
        task.abort();
    }
    Ok(())
}

#[tokio::test]
async fn unavailable_admission_preserves_the_exact_request_without_submission() -> Result<()> {
    let (temp, store, config, request) = fixture()?;
    let state = Arc::new(Mutex::new(Mock {
        unhealthy: true,
        ..Default::default()
    }));
    let (client, task) = server(temp.path(), Arc::clone(&state))?;
    assert!(run(&client, &store, &config, &request).await.is_err());
    assert_eq!(
        state
            .lock()
            .map_err(|e| anyhow::anyhow!("mock state lock: {e}"))?
            .submissions,
        0
    );
    assert_eq!(store.document(request.id.as_str())?.request, request);
    task.abort();
    Ok(())
}

#[derive(Default)]
struct BatchMock {
    jobs: std::collections::BTreeMap<String, Arc<Mutex<Mock>>>,
    seen: std::collections::BTreeSet<String>,
    active: std::collections::BTreeSet<String>,
    peak: usize,
    first_pair: Option<Arc<tokio::sync::Barrier>>,
}

async fn batch_respond(State(state): State<Arc<Mutex<BatchMock>>>, request: Request) -> Response {
    match batch_respond_inner(state, request).await {
        Ok(response) => response,
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn batch_respond_inner(state: Arc<Mutex<BatchMock>>, request: Request) -> Result<Response> {
    let path = request.uri().path().to_owned();
    let Some(id) = path.split('/').nth(3) else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let (job, barrier) = {
        let mut batch = state
            .lock()
            .map_err(|error| anyhow::anyhow!("fixture lock: {error}"))?;
        let Some(job) = batch.jobs.get(id).cloned() else {
            return Ok(StatusCode::NOT_FOUND.into_response());
        };
        let first = batch.seen.insert(id.to_owned());
        if first {
            batch.active.insert(id.to_owned());
            batch.peak = batch.peak.max(batch.active.len());
        }
        let barrier = if first && batch.seen.len() <= 2 {
            batch.first_pair.clone()
        } else {
            None
        };
        (job, barrier)
    };
    if let Some(barrier) = barrier {
        barrier.wait().await;
    }
    let response = respond(State(Arc::clone(&job)), request).await;
    let finished = {
        let job = job
            .lock()
            .map_err(|error| anyhow::anyhow!("fixture lock: {error}"))?;
        path.ends_with("/cancel")
            || path == format!("/v1/jobs/{id}") && job.replies.len() == job.calls.len()
    };
    if finished {
        state
            .lock()
            .map_err(|error| anyhow::anyhow!("fixture lock: {error}"))?
            .active
            .remove(id);
    }
    Ok(response)
}

#[tokio::test]
async fn one_runner_bounds_overlap_and_keeps_each_jobs_outcome() -> Result<()> {
    let (temp, store, config, first) = fixture()?;
    let mut requests = vec![first];
    for index in 1..4 {
        let request = request(&format!("Document {index}"), None, temp.path())?;
        store.create(&request)?;
        requests.push(request);
    }
    let mut batch = BatchMock {
        first_pair: Some(Arc::new(tokio::sync::Barrier::new(2))),
        ..Default::default()
    };
    for (index, request) in requests.iter().enumerate() {
        batch.jobs.insert(
            request.id.to_string(),
            Arc::new(Mutex::new(Mock {
                request: Some(request.clone()),
                calls: vec![call(
                    request,
                    "submit",
                    "submit_document",
                    json!({"markdown":format!("Document {index}")}),
                )?],
                terminal: if index == 2 { "failed" } else { "completed" },
                started_at: Some(
                    (OffsetDateTime::now_utc()
                        - time::Duration::seconds(if index == 0 { 1801 } else { 0 }))
                    .format(&Rfc3339)?,
                ),
                ..Default::default()
            })),
        );
    }
    let state = Arc::new(Mutex::new(batch));
    let socket = temp.path().join("batch.sock");
    let listener = tokio::net::UnixListener::bind(&socket)?;
    let router = Router::new()
        .fallback(any(batch_respond))
        .with_state(Arc::clone(&state));
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    let client = NucleusClient::new(socket)?;
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        crate::operations::run_many(&client, &store, &config, &requests, 2),
    )
    .await?;
    assert_eq!(result.results.len(), 4);
    for (index, item) in result.results.iter().enumerate() {
        assert_eq!(item.id, requests[index].id.as_str());
        assert_eq!(item.error.is_some(), index == 0 || index == 2);
        assert_eq!(item.result.is_some(), index == 1 || index == 3);
        assert_eq!(
            store.document(&item.id)?.markdown,
            (index != 0).then(|| format!("Document {index}"))
        );
    }
    assert!(
        result.results[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("deadline"))
    );
    {
        let batch = state
            .lock()
            .map_err(|error| anyhow::anyhow!("fixture lock: {error}"))?;
        assert_eq!(batch.peak, 2);
        assert!(batch.active.is_empty());
        for (index, request) in requests.iter().enumerate() {
            let job = batch.jobs[request.id.as_str()]
                .lock()
                .map_err(|error| anyhow::anyhow!("fixture lock: {error}"))?;
            assert_eq!(job.cancellations, usize::from(index == 0));
        }
    }
    server.abort();
    Ok(())
}

#[tokio::test]
async fn queue_time_is_free_and_resume_uses_the_original_job_start() -> Result<()> {
    let (temp, store, config, request) = fixture()?;
    let now = OffsetDateTime::now_utc();
    let state = Arc::new(Mutex::new(Mock {
        request: Some(request.clone()),
        calls: vec![call(
            &request,
            "submit",
            "submit_document",
            json!({"markdown":"Saved"}),
        )?],
        terminal: "completed",
        queued: 1,
        started_at: Some((now - time::Duration::seconds(600)).format(&Rfc3339)?),
        ..Default::default()
    }));
    let (client, server) = server(temp.path(), Arc::clone(&state))?;
    let queued = client.get_job(&request.id).await?;
    assert_eq!(
        remaining_wait(&queued, now + time::Duration::hours(5))?,
        None
    );
    let running = client.get_job(&request.id).await?;
    assert_eq!(
        remaining_wait(&running, now)?,
        Some(Duration::from_secs(1200))
    );
    assert_eq!(
        remaining_wait(&running, now + time::Duration::seconds(300))?,
        Some(Duration::from_secs(900))
    );
    run(&client, &store, &config, &request).await?;
    state
        .lock()
        .map_err(|error| anyhow::anyhow!("fixture lock: {error}"))?
        .started_at = Some((now - time::Duration::hours(2)).format(&Rfc3339)?);
    run(&client, &store, &config, &request).await?;
    assert_eq!(
        state
            .lock()
            .map_err(|error| anyhow::anyhow!("fixture lock: {error}"))?
            .cancellations,
        0
    );
    server.abort();
    Ok(())
}
