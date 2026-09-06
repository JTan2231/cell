use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use nucleus_core::JobRequestV1;
use serde_json::{Value, json};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[test]
fn missing_output_fails_the_canary_without_writing_or_retrying_a_stage() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let socket = temporary.path().join("nucleus.sock");
    let listener = UnixListener::bind(&socket)?;
    listener.set_nonblocking(true)?;
    let server = thread::spawn(move || serve_missing_output(&listener));
    let directory = temporary.path().join("canary");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_weaver"))
            .env("NUCLEUS_SOCKET", &socket)
            .env_remove("CELL_DEPLOYMENT_RUN_ID")
            .args(["maintenance", "canary", "--directory"])
            .arg(&directory)
            .output()
    };
    let result = run()?;
    match server.join() {
        Ok(result) => result?,
        Err(_) => return Err("fake Nucleus server panicked".into()),
    }
    assert!(!result.status.success());
    assert!(String::from_utf8(result.stderr)?.contains("no structured attempt output"));
    let current_path = directory.join("state/current.json");
    let retained = std::fs::read(&current_path)?;
    let current: Value = serde_json::from_slice(&retained)?;
    assert_eq!(current["status"], "failed");
    assert_eq!(current["nextStage"], 0);
    assert!(current.get("activeRequest").is_none());
    assert!(current.get("activeJobId").is_none());
    for stage in weaver::api::STAGES {
        assert!(
            !directory
                .join("narratives/canary")
                .join(stage.directory)
                .join("output.md")
                .exists()
        );
    }

    // The fake listener is gone: a repeated canary must use retained terminal
    // state without admitting a replacement job or changing the failed run.
    let repeated = run()?;
    assert!(!repeated.status.success());
    assert!(String::from_utf8(repeated.stderr)?.contains("did not produce five verified outputs"));
    assert_eq!(std::fs::read(current_path)?, retained);
    Ok(())
}

fn serve_missing_output(listener: &UnixListener) -> TestResult {
    let mut job = None;
    for step in 0..4 {
        let (mut stream, request_line, body) = read_request(listener)?;
        let (status, response) = match step {
            0 => {
                assert!(request_line.starts_with("GET /v1/health "));
                (
                    "200 OK",
                    json!({
                        "version": 1, "status": "ok", "daemonVersion": "fixture",
                        "acceptingJobs": true, "checkedAt": "2026-09-01T00:00:00Z",
                        "supportedProtocolVersions": [1],
                        "harness": {"harness": "codex", "harnessVersion": "0.146.0", "adapterVersion": "fixture"},
                        "harnessExecutable": "/tmp/fixture-codex",
                        "capabilities": ["exact-model", "reasoning-effort", "workspace-read-only", "builtin-local-execution", "builtin-web-search"],
                        "authentication": {"codexHome": "/tmp/fixture-codex-home", "configured": true, "authenticated": true}
                    }),
                )
            }
            1 => {
                assert!(request_line.starts_with("GET /v1/jobs/"));
                (
                    "404 Not Found",
                    json!({"version": 1, "code": "not_found", "message": "No prior fixture job"}),
                )
            }
            2 => {
                assert!(request_line.starts_with("POST /v1/jobs "));
                let request: JobRequestV1 = serde_json::from_slice(&body)?;
                let digest = request.request_digest()?;
                let response = json!({
                    "version": 1, "jobId": request.id, "state": "accepted",
                    "requestDigest": digest, "logCursor": 0
                });
                job = Some(json!({
                    "version": 1,
                    "summary": {
                        "version": 1, "id": request.id, "label": request.label,
                        "requester": request.requester, "state": "completed", "requestDigest": digest,
                        "createdAt": "2026-09-01T00:00:00Z", "updatedAt": "2026-09-01T00:00:01Z",
                        "completedAt": "2026-09-01T00:00:01Z", "currentAttemptId": "attempt-1"
                    },
                    "request": request,
                    "attempts": [{
                        "version": 1, "id": "attempt-1", "jobId": request.id, "ordinal": 1,
                        "harness": {"harness": "codex", "harnessVersion": "0.146.0", "adapterVersion": "fixture"},
                        "state": "completed", "terminalReason": "completed",
                        "createdAt": "2026-09-01T00:00:00Z", "completedAt": "2026-09-01T00:00:01Z"
                    }]
                }));
                ("202 Accepted", response)
            }
            _ => {
                assert!(request_line.starts_with("GET /v1/jobs/"));
                ("200 OK", job.take().ok_or("missing fixture job")?)
            }
        };
        let bytes = serde_json::to_vec(&response)?;
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )?;
        stream.write_all(&bytes)?;
    }
    Ok(())
}

fn read_request(listener: &UnixListener) -> TestResult<(UnixStream, String, Vec<u8>)> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        if header == "\r\n" || header.is_empty() {
            break;
        }
        if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse()?;
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok((stream, line, body))
}
