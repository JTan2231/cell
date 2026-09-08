#![allow(clippy::unwrap_used)] // Fixture setup and expected successful operations must fail the test immediately.

use std::fs;
use std::path::Path;
use std::time::Duration;

use decisions::document::{Snapshot, classification_schema};
use nucleus_client::NucleusClient;
use nucleus_core::{JobRequestV1, SchemaId, ToolCallV1, WorkspaceAccess};
use serde_json::json;
use serde_json::value::to_raw_value;

use super::{INPUT_SCHEMA, Run, TOOL_NAME, build_request, execute, toolset};
use nucleus_core::{AttemptId, ToolCallId};
use serde_json::Value;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{UnixListener, UnixStream};

#[path = "../../tests/support/document_source.rs"]
mod support;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture() -> Run {
    let snapshot = Snapshot::capture(support::conversation(), "target").unwrap();
    let request = build_request(&snapshot).unwrap();
    Run {
        version: 1,
        snapshot,
        request,
        receipts: Vec::new(),
        terminal: false,
    }
}

fn call(run: &Run, arguments: &Value) -> ToolCallV1 {
    ToolCallV1 {
        version: 1,
        id: ToolCallId::new("call-1"),
        job_id: run.request.id.clone(),
        attempt_id: AttemptId::new("attempt-1"),
        request_sequence: 1,
        tool_name: TOOL_NAME.to_owned(),
        arguments_schema_id: SchemaId::new(INPUT_SCHEMA),
        arguments: to_raw_value(arguments).unwrap(),
    }
}

#[test]
fn receipt_replay_and_render_recovery_use_frozen_source_and_result() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut run = fixture();
    let call = call(
        &run,
        &json!({"is_decision": true, "summary": "No tests, builds, or formatters were run."}),
    );
    run.save(directory.path())?;
    let result = run.accept_call(directory.path(), &call)?;
    assert!(!result.is_error);
    assert!(!directory.path().join("decision.md").exists());
    let mut restored = Run::read(directory.path())?;
    assert_eq!(restored.request.digest()?, run.request.digest()?);
    let replay = restored.accept_call(directory.path(), &call)?;
    assert_eq!(serde_json::to_vec(&replay)?, serde_json::to_vec(&result)?);
    assert_eq!(restored.receipts.len(), 1);
    let path = restored.render(directory.path())?.unwrap();
    let expected = fs::read(&path)?;
    fs::remove_file(&path)?;
    restored.render(directory.path())?;
    assert_eq!(fs::read(&path)?, expected);
    let mut conflict = call.clone();
    conflict.arguments = to_raw_value(&json!({"is_decision": false, "summary": null}))?;
    assert_eq!(
        restored
            .accept_call(directory.path(), &conflict)
            .unwrap_err()
            .code,
        "document_call_conflict"
    );
    fs::write(&path, "user-edited document")?;
    assert_eq!(
        restored.render(directory.path()).unwrap_err().code,
        "document_conflict"
    );
    assert_eq!(fs::read_to_string(path)?, "user-edited document");
    Ok(())
}

#[test]
fn malformed_results_explain_the_error_and_allow_a_corrected_call() -> TestResult {
    let directory = tempfile::tempdir()?;
    let mut run = fixture();
    let invalid = call(&run, &json!({"is_decision": true, "summary": null}));
    let result = run.accept_call(directory.path(), &invalid)?;
    assert!(result.is_error);
    assert!(
        result
            .result
            .get()
            .contains("nonblank, single-line summary")
    );
    assert!(Run::read(directory.path())?.classification().is_none());
    let mut corrected = call(&run, &json!({"is_decision": false, "summary": null}));
    corrected.id = ToolCallId::new("call-2");
    corrected.request_sequence = 2;
    assert!(!run.accept_call(directory.path(), &corrected)?.is_error);
    assert_eq!(run.render(directory.path())?, None);
    assert!(!directory.path().join("decision.md").exists());
    Ok(())
}

#[test]
fn request_uses_new_contract_and_only_one_generated_summary() {
    let run = fixture();
    assert_eq!(run.request.invocation.toolset, Some(toolset()));
    assert_eq!(
        run.request.invocation.workspace_access,
        WorkspaceAccess::None
    );
    assert!(!run.request.invocation.builtin_tools.local_execution);
    assert!(!run.request.invocation.builtin_tools.web_search);
    assert!(run.request.prompt.contains("/tmp/config"));
    assert!(
        run.request
            .prompt
            .contains("No tests, builds, or formatters were run.")
    );
    assert!(!run.request.instructions.contains("privacy"));
    assert_eq!(
        classification_schema()["required"],
        json!(["is_decision", "summary"])
    );
}

async fn read_request(stream: &mut UnixStream) -> std::io::Result<(String, Value)> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if bytes.len() >= end + 4 + length {
                let body = if length == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes[end + 4..end + 4 + length])?
                };
                return Ok((headers.lines().next().unwrap().to_owned(), body));
            }
        }
    }
}

async fn respond(stream: &mut UnixStream, status: &str, value: &Value) -> std::io::Result<()> {
    let body = value.to_string();
    stream.write_all(format!("HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes()).await
}

fn health() -> Value {
    json!({"version":1,"status":"ok","daemonVersion":"0.1.0","acceptingJobs":true,
        "checkedAt":"2026-09-08T00:00:00Z","supportedProtocolVersions":[1],
        "harness":{"harness":"codex","harnessVersion":"test","adapterVersion":"test"},
        "harnessExecutable":"/usr/bin/false",
        "capabilities":["exact-model","reasoning-effort","workspace-none","dynamic-client-tools","developer-instructions","persistent-file-authentication"],
        "execution":{"maxActiveJobs":8,"activeJobs":8,"availableSlots":0},
        "authentication":{"codexHome":"/tmp/codex-home","configured":true,"authenticated":true}})
}

/// The first run loses the acknowledgement. A restart receives the same call, then a failed job.
async fn serve_run(
    listener: &UnixListener,
    directory: &Path,
    request: &JobRequestV1,
    tool_call: Option<&ToolCallV1>,
    lose_ack: bool,
) -> TestResult {
    let mut queued = lose_ack;
    loop {
        let (mut stream, _) =
            tokio::time::timeout(Duration::from_secs(5), listener.accept()).await??;
        let (route, body) = read_request(&mut stream).await?;
        let mut status = "200 OK";
        let mut done = false;
        let value = if route.starts_with("GET /v1/health ") {
            health()
        } else if route.starts_with("POST /v1/schemas ") {
            body
        } else if route.starts_with("POST /v1/toolsets ") {
            json!({"version":1,"toolset":toolset(),"definitionsSchemaId":"nucleus.toolset-definitions.v1","digest":"test","registeredAt":"2026-09-08T00:00:00Z"})
        } else if route.starts_with("POST /v1/jobs ") {
            assert_eq!(body, serde_json::to_value(request)?);
            assert_eq!(Run::read(directory)?.request.digest()?, request.digest()?);
            json!({"version":1,"jobId":request.id,"state":"accepted","requestDigest":request.digest()?,"logCursor":0})
        } else if route.contains("/tool-calls/") {
            let saved = Run::read(directory)?;
            let result = saved.receipts.last().unwrap();
            assert_eq!(body, serde_json::to_value(&result.result)?);
            assert!(directory.join("decision.md").exists());
            status = if lose_ack {
                "500 Internal Server Error"
            } else {
                "409 Conflict"
            };
            done = lose_ack;
            json!({"version":1,"code":if lose_ack {"test_lost_ack"} else {"job_terminal"},"message":"test","issues":[]})
        } else if route.contains("/tool-calls?") {
            let calls = if queued {
                vec![]
            } else {
                tool_call.iter().map(|call| json!({"version":1,"call":call,"state":"pending","createdAt":"2026-09-08T00:00:00Z"})).collect::<Vec<_>>()
            };
            json!({"version":1,"jobId":request.id,"calls":calls,"nextSequence":1})
        } else if route.starts_with(&format!("GET /v1/jobs/{} ", request.id)) {
            let state = if queued {
                "accepted"
            } else if tool_call.is_some() {
                "failed"
            } else {
                "completed"
            };
            done = !queued;
            queued = false;
            json!({"version":1,"summary":{"version":1,"id":request.id,"label":"test","requester":request.requester,
                "state":state,"requestDigest":request.digest()?,"createdAt":"2026-09-08T00:00:00Z","updatedAt":"2026-09-08T00:00:01Z"},
                "request":request,"attempts":[]})
        } else {
            panic!("unexpected request: {route}")
        };
        respond(&mut stream, status, &value).await?;
        if done {
            return Ok(());
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn restart_replays_same_request_and_durable_result_despite_runtime_failure() -> TestResult {
    let directory = tempfile::tempdir()?;
    let socket = directory.path().join("n.sock");
    let listener = UnixListener::bind(&socket)?;
    let client = NucleusClient::new(&socket)?;
    let mut run = fixture();
    run.save(directory.path())?;
    let request = run.request.clone();
    let tool_call = call(
        &run,
        &json!({"is_decision":true,"summary":"Store the API token in /tmp/config."}),
    );
    let (result, server) = tokio::join!(
        execute(&client, &mut run, directory.path()),
        serve_run(
            &listener,
            directory.path(),
            &request,
            Some(&tool_call),
            true
        )
    );
    server?;
    assert_eq!(result.unwrap_err().code, "nucleus_acknowledgement_failed");
    let mut restored = Run::read(directory.path())?;
    assert!(!restored.terminal);
    assert!(restored.classification().is_some());
    let (result, server) = tokio::join!(
        execute(&client, &mut restored, directory.path()),
        serve_run(
            &listener,
            directory.path(),
            &request,
            Some(&tool_call),
            false
        )
    );
    server?;
    result?;
    assert!(restored.terminal);
    assert_eq!(restored.receipts.len(), 1);
    assert!(Run::read(directory.path())?.terminal);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn runtime_completion_without_a_classification_does_not_produce_a_document() -> TestResult {
    let directory = tempfile::tempdir()?;
    let socket = directory.path().join("n.sock");
    let listener = UnixListener::bind(&socket)?;
    let client = NucleusClient::new(&socket)?;
    let mut run = fixture();
    run.save(directory.path())?;
    let request = run.request.clone();
    let (result, server) = tokio::join!(
        execute(&client, &mut run, directory.path()),
        serve_run(&listener, directory.path(), &request, None, false)
    );
    server?;
    assert_eq!(result.unwrap_err().code, "document_classification_missing");
    assert!(!directory.path().join("decision.md").exists());
    assert!(run.terminal);
    Ok(())
}
