//! Real-harness compatibility proof without a model service or real credentials.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nucleus_codex::runtime_bundle::{RUNTIME_MANIFEST, stage_runtime, verify_runtime};
use nucleus_codex::{BuiltinToolsV1, CodexEvent, CodexHarness, CodexRunSpec, WorkspaceAccess};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const MARKER: &str = "nucleus-real-local-tool-result";

#[tokio::test]
async fn exact_codex_executes_a_real_local_tool() -> TestResult {
    let executable = test_codex_path()?;
    let directory = tempfile::tempdir()?;
    let bundle = if executable
        .parent()
        .is_some_and(|path| path.join(RUNTIME_MANIFEST).exists())
    {
        verify_runtime(&executable)?
    } else {
        let runtime = directory.path().join("runtime");
        let staged = stage_runtime(&executable, &runtime)?;
        // Only this disposable directory must remain removable by TempDir.
        fs::set_permissions(runtime, fs::Permissions::from_mode(0o700))?;
        staged
    };
    let executable = bundle.executable;
    let home = directory.path().join("home");
    let credentials = directory.path().join("credentials");
    fs::create_dir(&home)?;
    write_fake_credentials(&credentials)?;
    let marker_path = home.join("local-tool-result");

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let base_url = format!("http://{}/v1", listener.local_addr()?);
    let script = format!(
        "const result = await tools.exec_command({}); text(result.output);",
        json!({
            "cmd": format!("printf '%s' '{MARKER}' > local-tool-result; cat local-tool-result"),
            "workdir": home,
            "login": false,
            "yield_time_ms": 1000,
            "max_output_tokens": 1000,
        })
    );
    let server = tokio::spawn(serve_responses(listener, script));
    let real_harness = CodexHarness::with_codex_home(&executable, &credentials);
    let mut inspection = real_harness.inspect().await?;
    let schema = real_harness.generate_protocol_schema().await?;
    real_harness.validate_protocol_schema(&inspection, &schema)?;
    let launcher = directory.path().join("offline-codex");
    write_offline_launcher(&launcher, &executable, &base_url)?;
    inspection.executable.clone_from(&launcher);
    let harness = CodexHarness::with_codex_home(launcher, credentials);
    let environment = BTreeMap::from([
        ("HOME".to_owned(), home.display().to_string()),
        (
            "PATH".to_owned(),
            "/usr/bin:/bin:/usr/sbin:/sbin".to_owned(),
        ),
        ("HTTPS_PROXY".to_owned(), "http://127.0.0.1:9".to_owned()),
        ("HTTP_PROXY".to_owned(), "http://127.0.0.1:9".to_owned()),
        ("ALL_PROXY".to_owned(), "http://127.0.0.1:9".to_owned()),
        ("NO_PROXY".to_owned(), "127.0.0.1,localhost".to_owned()),
    ]);
    let spec = CodexRunSpec {
        instructions: "Execute the requested local command, then report its result.".to_owned(),
        developer_instructions: None,
        prompt: "Perform the local execution compatibility check.".to_owned(),
        model: "gpt-6-astra".to_owned(),
        reasoning_effort: Some("low".to_owned()),
        working_directory: home,
        workspace_access: WorkspaceAccess::Unrestricted,
        builtin_tools: BuiltinToolsV1 {
            local_execution: true,
            web_search: false,
        },
        timeout: Duration::from_secs(20),
        tools: Vec::new(),
        launch_environment: Some(environment),
    };
    let (events_tx, mut events_rx) = mpsc::channel(256);
    let events = tokio::spawn(async move {
        let mut retained = Vec::new();
        while let Some(event) = events_rx.recv().await {
            match event {
                CodexEvent::Protocol { bytes, .. } | CodexEvent::Stderr(bytes) => {
                    retained.push(String::from_utf8_lossy(&bytes).into_owned());
                }
                CodexEvent::ToolCall(call) => {
                    retained.push(format!("unexpected requester tool {}", call.name));
                }
            }
        }
        retained
    });
    let (_cancel_tx, cancel_rx) = watch::channel(false);
    let outcome = harness.run(&inspection, spec, events_tx, cancel_rx).await;
    let retained = events.await?;
    let outcome = outcome.map_err(|error| format!("{error}\n{}", retained.join("\n")))?;
    let observed = tokio::time::timeout(Duration::from_secs(2), server).await???;
    assert_eq!(outcome.final_message, "Local command verified.");
    assert_eq!(fs::read_to_string(marker_path)?, MARKER);
    assert!(
        observed,
        "the actual tool result did not reach the next model request"
    );
    Ok(())
}

fn test_codex_path() -> TestResult<PathBuf> {
    std::env::var_os("NUCLEUS_TEST_CODEX")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                PathBuf::from(home)
                    .join("Library/Application Support/Nucleus/harnesses/codex")
                    .join(nucleus_codex::SUPPORTED_CODEX_VERSION)
                    .join("runtime/codex")
            })
        })
        .ok_or_else(|| "HOME or NUCLEUS_TEST_CODEX is required for the real runtime test".into())
}

fn write_fake_credentials(directory: &Path) -> TestResult {
    fs::create_dir(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    let config = directory.join("config.toml");
    fs::write(&config, "cli_auth_credentials_store = \"file\"\n")?;
    fs::set_permissions(&config, fs::Permissions::from_mode(0o600))?;
    let auth = directory.join("auth.json");
    fs::write(&auth, br#"{"OPENAI_API_KEY":"nucleus-offline-test-key"}"#)?;
    fs::set_permissions(&auth, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn write_offline_launcher(launcher: &Path, executable: &Path, base_url: &str) -> TestResult {
    // The adapter deliberately does not expose provider overrides to requesters.
    // This launcher changes only the model transport, then execs the exact
    // inspected binary with every ordinary Nucleus argument unchanged.
    let settings = [
        "model_provider=\"offline\"".to_owned(),
        "model_providers.offline.name=\"Offline compatibility test\"".to_owned(),
        format!("model_providers.offline.base_url={}", json!(base_url)),
        "model_providers.offline.wire_api=\"responses\"".to_owned(),
        "model_providers.offline.requires_openai_auth=false".to_owned(),
        "model_providers.offline.request_max_retries=0".to_owned(),
        "model_providers.offline.stream_max_retries=0".to_owned(),
    ];
    let quoted = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let mut script = format!(
        "#!/bin/sh\nexec {}",
        quoted(&executable.display().to_string())
    );
    for value in settings {
        write!(&mut script, " -c {}", quoted(&value))?;
    }
    script.push_str(" \"$@\"\n");
    fs::write(launcher, script)?;
    fs::set_permissions(launcher, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

async fn serve_responses(listener: TcpListener, script: String) -> TestResult<bool> {
    let mut saw_result = false;
    for turn in 0..2 {
        let (mut stream, _) = listener.accept().await?;
        let request = read_request(&mut stream).await?;
        if turn == 1 {
            saw_result = request["input"].as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item["type"] == "custom_tool_call_output"
                        && item["call_id"] == "local-check"
                        && item["output"].to_string().contains(MARKER)
                })
            });
        }
        let item = if turn == 0 {
            json!({"type":"custom_tool_call", "id":"tool-item", "call_id":"local-check", "name":"exec", "input":script, "status":"completed"})
        } else {
            json!({"type":"message", "id":"final-item", "role":"assistant", "status":"completed", "content":[{"type":"output_text", "text":"Local command verified.", "annotations":[]}]})
        };
        let response_id = format!("response-{turn}");
        let events = [
            json!({"type":"response.created", "response":{"id":response_id,"status":"in_progress","output":[]}}),
            json!({"type":"response.output_item.done", "output_index":0, "item":item}),
            json!({"type":"response.completed", "response":{"id":response_id,"status":"completed","output":[item],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}),
        ];
        let mut body = String::new();
        for event in events {
            write!(&mut body, "data: {event}\n\n")?;
        }
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(headers.as_bytes()).await?;
        stream.write_all(body.as_bytes()).await?;
        stream.shutdown().await?;
    }
    Ok(saw_result)
}

async fn read_request(stream: &mut TcpStream) -> TestResult<Value> {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut buffer = [0_u8; 8192];
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Err("request ended before headers".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(index) = bytes.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break index + 4;
        }
        if bytes.len() > 1024 * 1024 {
            return Err("request headers exceeded limit".into());
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end])?;
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>())
        })
        .ok_or("request omitted content length")??;
    if length > 4 * 1024 * 1024 {
        return Err("request body exceeded limit".into());
    }
    while bytes.len() < header_end + length {
        let mut buffer = [0_u8; 8192];
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Err("request ended before body".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok(serde_json::from_slice(
        &bytes[header_end..header_end + length],
    )?)
}
