use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{
    AbsolutePath, AccountSnapshotQueryV1, AgentInvocationV1, BuiltinToolsV1, ErrorResponseV1,
    JobId, JobRequestV1, JobState, LaunchContextRegistrationV1, LaunchEnvironmentVariableV1,
    LifecycleEventKind, LifecycleEventV1, ListJobsQueryV1, LogSchemaV1, LogStream, LogsQueryV1,
    PROTOCOL_VERSION_V1, Requester, SchemaId, TimeoutSeconds, ToolCallState, ToolCallsQueryV1,
    ToolDefinitionV1, ToolResultV1, ToolsetDefinitionsV1, ToolsetRef, ToolsetRegistrationV1,
    WorkspaceAccess, sha256_digest,
};
use serde_json::value::to_raw_value;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::process::{Child, Command};

const ORIGIN: &str = "http://nucleus.local";
const FAKE_MODEL: &str = "fake-model";
const SUPPORTED_CODEX_VERSION: &str = "0.146.0";
const COMPATIBLE_PROTOCOL_SCHEMA: &str = r##"{
  "definitions": {
    "ClientRequest": {"oneOf": [
      {"properties":{"method":{"enum":["initialize"]},"params":{"$ref":"#/definitions/InitializeParams"}}},
      {"properties":{"method":{"enum":["thread/start"]},"params":{"$ref":"#/definitions/v2/ThreadStartParams"}}},
      {"properties":{"method":{"enum":["turn/start"]},"params":{"$ref":"#/definitions/v2/TurnStartParams"}}},
      {"properties":{"method":{"enum":["mcpServerStatus/list"]},"params":{"$ref":"#/definitions/v2/ListMcpServerStatusParams"}}}
    ]},
    "ServerRequest": {"oneOf": [
      {"properties":{"method":{"enum":["item/tool/call"]},"params":{"$ref":"#/definitions/DynamicToolCallParams"}}}
    ]},
    "ServerNotification": {"oneOf": [
      {"properties":{"method":{"enum":["item/completed"]},"params":{"$ref":"#/definitions/v2/ItemCompletedNotification"}}},
      {"properties":{"method":{"enum":["turn/completed"]},"params":{"$ref":"#/definitions/v2/TurnCompletedNotification"}}},
      {"properties":{"method":{"enum":["thread/tokenUsage/updated"]},"params":{"$ref":"#/definitions/v2/ThreadTokenUsageUpdatedNotification"}}}
    ]},
    "DynamicToolCallParams": {"required":["arguments","callId","threadId","tool","turnId"]},
    "v2": {
      "ThreadStartParams": {"properties":{
        "approvalPolicy":{},"baseInstructions":{},"cwd":{},"developerInstructions":{},"dynamicTools":{},"ephemeral":{},"environments":{},"experimentalRawEvents":{},"model":{},"sandbox":{}
      }},
      "AskForApproval": {"enum":["never"]},
      "SandboxMode": {"enum":["read-only","workspace-write"]},
      "TurnStartParams": {
        "required":["input","threadId"],
        "properties":{"effort":{},"environments":{},"input":{},"threadId":{}}
      },
      "DynamicToolSpec": {"oneOf":[{
        "required":["description","inputSchema","name","type"],
        "properties":{"type":{"enum":["function"]}}
      }]},
      "ThreadStartResponse": {"required":["thread"]},
      "TurnStartResponse": {"required":["turn"]},
      "Thread": {"required":["id"]},
      "Turn": {"required":["id","status"]},
      "TurnStatus": {"enum":["completed","failed"]},
      "ItemCompletedNotification": {"required":["item","threadId","turnId"]},
      "TurnCompletedNotification": {"required":["threadId","turn"]},
      "ThreadTokenUsageUpdatedNotification": {"required":["threadId","tokenUsage","turnId"]},
      "RawResponseCompletedNotification": {"required":["responseId","threadId","turnId"]}
    }
  }
}"##;

trait TestValueExt<T> {
    fn or_panic(self, context: &str) -> T;
}

impl<T, E> TestValueExt<T> for Result<T, E>
where
    E: std::fmt::Debug,
{
    fn or_panic(self, context: &str) -> T {
        match self {
            Ok(value) => value,
            Err(error) => panic!("{context}: {error:?}"),
        }
    }
}

impl<T> TestValueExt<T> for Option<T> {
    fn or_panic(self, context: &str) -> T {
        match self {
            Some(value) => value,
            None => panic!("{context}"),
        }
    }
}

struct DaemonFixture {
    temporary: TempDir,
    child: Child,
    client: NucleusClient,
    raw_client: reqwest::Client,
    stderr_path: PathBuf,
}

impl DaemonFixture {
    async fn start() -> Self {
        Self::start_with(SUPPORTED_CODEX_VERSION, COMPATIBLE_PROTOCOL_SCHEMA).await
    }

    async fn start_with(codex_version: &str, protocol_schema: &str) -> Self {
        let temporary = tempfile::tempdir().or_panic("create test directory");
        let root = temporary.path();
        let socket = root.join("nucleus.sock");
        let database = root.join("nucleus.db");
        let fake_codex = root.join("fake-codex");
        let home = root.join("home");
        let codex_home = home.join(".codex");
        fs::create_dir_all(&codex_home).or_panic("create fake Codex home");
        fs::set_permissions(&codex_home, fs::Permissions::from_mode(0o700))
            .or_panic("secure fake Codex home");
        fs::write(
            codex_home.join("config.toml"),
            b"cli_auth_credentials_store = \"file\"\n",
        )
        .or_panic("write fake config");
        fs::set_permissions(
            codex_home.join("config.toml"),
            fs::Permissions::from_mode(0o600),
        )
        .or_panic("secure fake config");
        fs::write(
            codex_home.join("auth.json"),
            br#"{"OPENAI_API_KEY":"fixture"}"#,
        )
        .or_panic("write fake auth");
        fs::set_permissions(
            codex_home.join("auth.json"),
            fs::Permissions::from_mode(0o600),
        )
        .or_panic("secure fake auth");
        write_fake_codex(&fake_codex, codex_version, protocol_schema);

        let stderr_path = root.join("nucleusd.stderr.log");
        let stdout_path = root.join("nucleusd.stdout.log");
        let stderr = File::create(&stderr_path).or_panic("create daemon stderr log");
        let stdout = File::create(stdout_path).or_panic("create daemon stdout log");
        let mut child = Command::new(env!("CARGO_BIN_EXE_nucleusd"))
            .args(["serve", "--socket"])
            .arg(&socket)
            .arg("--database")
            .arg(&database)
            .arg("--codex")
            .arg(&fake_codex)
            .arg("--codex-home")
            .arg(&codex_home)
            .env("HOME", &home)
            .env("CODEX_HOME", &codex_home)
            .env("RUST_LOG", "nucleus=debug")
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .kill_on_drop(true)
            .spawn()
            .or_panic("start nucleusd");

        let client = NucleusClient::new(&socket).or_panic("build typed client");
        let raw_client = reqwest::Client::builder()
            .http1_only()
            .unix_socket(socket.clone())
            .build()
            .or_panic("build raw client");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(Ok(_)) =
                tokio::time::timeout(Duration::from_millis(500), client.health()).await
            {
                break;
            }
            if let Some(status) = child.try_wait().or_panic("inspect daemon process") {
                let diagnostics = fs::read_to_string(&stderr_path).unwrap_or_default();
                panic!("nucleusd exited during startup ({status}): {diagnostics}");
            }
            assert!(
                Instant::now() < deadline,
                "nucleusd did not become reachable"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        Self {
            temporary,
            child,
            client,
            raw_client,
            stderr_path,
        }
    }

    async fn shutdown(mut self) {
        self.child.start_kill().or_panic("signal test daemon");
        tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .or_panic("test daemon should stop promptly")
            .or_panic("wait for test daemon");
    }

    fn request(&self, id: &str, requester: Requester, prompt: &str) -> JobRequestV1 {
        JobRequestV1::new(
            id,
            format!("contract test {id}"),
            requester,
            "Follow the contract test instructions and use only admitted tools.",
            prompt,
            AgentInvocationV1::new(
                "codex",
                FAKE_MODEL,
                AbsolutePath::new(self.temporary.path()),
                WorkspaceAccess::None,
                BuiltinToolsV1 {
                    local_execution: false,
                    web_search: false,
                },
                TimeoutSeconds::new(30),
            ),
        )
    }

    fn diagnostics(&self) -> String {
        fs::read_to_string(&self.stderr_path).unwrap_or_default()
    }
}

impl Drop for DaemonFixture {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn daemon_http_contract_is_strict_durable_and_attributed() {
    let fixture = DaemonFixture::start().await;

    let health = fixture.client.health().await.or_panic("read daemon health");
    assert_eq!(health.version, PROTOCOL_VERSION_V1);
    assert_eq!(health.status, "ok");
    assert!(health.accepting_jobs);
    assert!(health.authentication.authenticated);
    let account = fixture
        .client
        .account_snapshot(&AccountSnapshotQueryV1 {
            include_usage: false,
            wait_seconds: 0,
        })
        .await
        .or_panic("read Nucleus-owned account snapshot");
    assert_eq!(account.rate_limits, json!({ "data": [] }));

    let requester = Requester {
        program: "todo".to_owned(),
        id: "todo-contract-42".to_owned(),
    };
    let mut request = fixture.request("job-contract-success", requester.clone(), "COMPLETE");
    request.developer_instructions =
        Some("Keep the base contract and this rule separate.".to_owned());
    assert_strict_request_rejection(&fixture, &request).await;

    let schema_json = to_raw_value(&json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["answer"],
        "properties": { "answer": { "type": "string" } }
    }))
    .or_panic("encode requester result schema");
    let schema = LogSchemaV1 {
        version: PROTOCOL_VERSION_V1,
        id: SchemaId::new("todo.result.v1"),
        name: "Todo result".to_owned(),
        schema_version: "1".to_owned(),
        media_type: "application/schema+json".to_owned(),
        producer: "todo".to_owned(),
        producer_version: Some("test".to_owned()),
        digest: sha256_digest(schema_json.get().as_bytes()),
        schema: schema_json,
    };
    let registered_schema = fixture
        .client
        .register_schema(&schema)
        .await
        .or_panic("register requester schema");
    assert_eq!(registered_schema.id, schema.id);
    assert_eq!(registered_schema.digest, schema.digest);
    let idempotent_schema = fixture
        .client
        .register_schema(&schema)
        .await
        .or_panic("register requester schema idempotently");
    assert_eq!(idempotent_schema.digest, schema.digest);
    let fetched_schema = fixture
        .client
        .get_schema(&schema.id)
        .await
        .or_panic("fetch requester schema");
    assert_eq!(fetched_schema.schema.get(), schema.schema.get());

    let input_schema = to_raw_value(&json!({
        "type": "object",
        "required": ["todoId"],
        "properties": { "todoId": { "type": "string" } },
        "additionalProperties": false
    }))
    .or_panic("encode tool input schema");
    let toolset_ref = ToolsetRef {
        provider: "todo".to_owned(),
        name: "contract-tools".to_owned(),
        version: 1,
    };
    let toolset = ToolsetRegistrationV1::new(
        toolset_ref.clone(),
        "nucleus.toolset-definitions.v1",
        ToolsetDefinitionsV1 {
            version: PROTOCOL_VERSION_V1,
            tools: vec![ToolDefinitionV1 {
                name: "read_todo".to_owned(),
                description: "Read one todo for the deterministic contract test".to_owned(),
                input_schema_id: SchemaId::new("todo.tool.read-todo.input.v1"),
                input_schema,
            }],
        },
    )
    .or_panic("construct toolset registration");
    let registered_toolset = fixture
        .client
        .register_toolset(&toolset)
        .await
        .or_panic("register toolset");
    assert_eq!(registered_toolset.toolset, toolset_ref);
    assert_eq!(registered_toolset.digest, toolset.digest);
    let idempotent_toolset = fixture
        .client
        .register_toolset(&toolset)
        .await
        .or_panic("register toolset idempotently");
    assert_eq!(idempotent_toolset.digest, toolset.digest);
    let fetched_toolset = fixture
        .client
        .get_toolset("todo", "contract-tools", 1)
        .await
        .or_panic("fetch toolset");
    assert_eq!(fetched_toolset.digest, toolset.digest);

    let accepted = fixture
        .client
        .submit_job(&request)
        .await
        .unwrap_or_else(|error| {
            panic!("submit successful job: {error}; {}", fixture.diagnostics())
        });
    assert_eq!(accepted.job_id, request.id);
    assert_eq!(
        accepted.request_digest,
        request.digest().or_panic("digest request")
    );

    let repeated = fixture
        .client
        .submit_job(&request)
        .await
        .or_panic("repeat identical request");
    assert_eq!(repeated.job_id, accepted.job_id);
    assert_eq!(repeated.request_digest, accepted.request_digest);

    let mut conflicting = request.clone();
    conflicting.label.push_str(" changed");
    match fixture.client.submit_job(&conflicting).await {
        Err(ClientError::Api { status, code, .. }) => {
            assert_eq!(status, 409);
            assert_eq!(code, "job_conflict");
        }
        result => panic!("expected an HTTP 409 job conflict, got {result:?}"),
    }

    let attributed = fixture
        .client
        .list_jobs(&ListJobsQueryV1 {
            requester_program: Some(requester.program.clone()),
            requester_id: Some(requester.id.clone()),
            limit: Some(100),
            ..ListJobsQueryV1::default()
        })
        .await
        .or_panic("list requester jobs");
    assert_eq!(attributed.jobs.len(), 1);
    assert_eq!(attributed.jobs[0].id, request.id);
    assert_eq!(attributed.jobs[0].requester, requester);
    let unrelated = fixture
        .client
        .list_jobs(&ListJobsQueryV1 {
            requester_program: Some("annals".to_owned()),
            requester_id: Some("unrelated".to_owned()),
            limit: Some(100),
            ..ListJobsQueryV1::default()
        })
        .await
        .or_panic("list unrelated requester jobs");
    assert!(unrelated.jobs.is_empty());

    let completed = wait_for_state(&fixture, &request.id, JobState::Completed).await;
    assert_eq!(completed.summary.requester, request.requester);
    assert_eq!(completed.attempts.len(), 1);
    let output = completed.attempts[0]
        .output
        .as_ref()
        .or_panic("completed attempt has structured output");
    assert_eq!(output.thread_id, "thread-fake");
    assert_eq!(output.turn_id, "turn-fake");
    assert_eq!(output.final_message, "fake completed");
    let logs = fixture
        .client
        .logs(
            &request.id,
            &LogsQueryV1 {
                after: 0,
                follow: false,
                limit: Some(1_000),
            },
        )
        .await
        .or_panic("read completed job logs");
    assert!(!logs.records.is_empty());
    assert_eq!(
        logs.next_sequence,
        logs.records.last().or_panic("a final log record").sequence
    );
    assert!(
        logs.records
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence)
    );
    assert!(
        logs.records
            .iter()
            .any(|record| record.stream == LogStream::HarnessInput)
    );
    assert!(
        logs.records
            .iter()
            .any(|record| record.stream == LogStream::HarnessOutput)
    );
    assert!(lifecycle_events(&logs.records).contains(&LifecycleEventKind::JobCompleted));
    let thread_start = protocol_request(&logs.records, "thread/start");
    assert_eq!(
        thread_start.pointer("/params/baseInstructions"),
        Some(&json!(request.instructions))
    );
    assert_eq!(
        thread_start.pointer("/params/developerInstructions"),
        Some(&json!(request.developer_instructions))
    );
    assert_eq!(
        thread_start.pointer("/params/experimentalRawEvents"),
        Some(&json!(true))
    );
    assert_eq!(
        thread_start.pointer("/params/environments"),
        Some(&json!([]))
    );
    let turn_start = protocol_request(&logs.records, "turn/start");
    assert_eq!(turn_start.pointer("/params/environments"), Some(&json!([])));
    for record in &logs.records {
        let stored_schema = fixture
            .client
            .get_schema(&record.schema_id)
            .await
            .or_panic("every log record references a registered schema");
        assert_eq!(stored_schema.id, record.schema_id);
    }

    let mut tool_request = fixture.request(
        "job-contract-tool",
        Requester {
            program: "todo".to_owned(),
            id: "todo-contract-tool-9".to_owned(),
        },
        "USE_TOOL",
    );
    tool_request.invocation.toolset = Some(toolset_ref);
    fixture
        .client
        .submit_job(&tool_request)
        .await
        .or_panic("submit tool-backed job");
    let pending = fixture
        .client
        .pending_tool_calls(
            &tool_request.id,
            &ToolCallsQueryV1 {
                after: 0,
                wait_seconds: 5,
            },
        )
        .await
        .or_panic("long-poll for tool call");
    assert_eq!(pending.calls.len(), 1);
    let call = &pending.calls[0];
    assert_eq!(call.state, ToolCallState::Pending);
    assert_eq!(call.call.tool_name, "read_todo");
    assert_eq!(
        call.call.arguments_schema_id.as_str(),
        "todo.tool.read-todo.input.v1"
    );
    assert_eq!(
        serde_json::from_str::<Value>(call.call.arguments.get()).or_panic("decode tool arguments"),
        json!({ "todoId": "todo-1" })
    );
    let pending_logs = fixture
        .client
        .logs(&tool_request.id, &LogsQueryV1::default())
        .await
        .or_panic("raw tool request is durable with mailbox projection");
    let raw_call = pending_logs
        .records
        .iter()
        .find(|record| record.sequence == call.call.request_sequence)
        .or_panic("tool request sequence names its raw log record");
    assert_eq!(raw_call.stream, LogStream::HarnessOutput);
    assert_eq!(
        serde_json::from_str::<Value>(raw_call.payload.get())
            .or_panic("decode raw tool request")
            .pointer("/params/callId")
            .and_then(Value::as_str),
        Some("call-1")
    );
    let result_payload =
        to_raw_value(&json!({ "answer": "fake todo result" })).or_panic("encode tool result");
    let answered = fixture
        .client
        .post_tool_result(
            &tool_request.id,
            &call.call.id,
            &ToolResultV1 {
                version: PROTOCOL_VERSION_V1,
                call_id: call.call.id.clone(),
                requester: tool_request.requester.clone(),
                result_schema_id: schema.id.clone(),
                result: result_payload,
                is_error: false,
            },
        )
        .await
        .or_panic("post schema-bound tool result");
    assert_eq!(answered.state, ToolCallState::Answered);
    assert!(answered.result_sequence.is_some());
    wait_for_state(&fixture, &tool_request.id, JobState::Completed).await;
    let tool_logs = fixture
        .client
        .logs(&tool_request.id, &LogsQueryV1::default())
        .await
        .or_panic("read tool-backed job logs");
    let tool_events = lifecycle_events(&tool_logs.records);
    assert!(tool_events.contains(&LifecycleEventKind::WaitingOnRequester));
    assert!(tool_events.contains(&LifecycleEventKind::ToolCallPending));
    assert!(tool_events.contains(&LifecycleEventKind::ToolCallAnswered));
    assert!(tool_events.contains(&LifecycleEventKind::JobCompleted));
    assert!(
        tool_logs
            .records
            .iter()
            .any(|record| record.stream == LogStream::Requester && record.schema_id == schema.id)
    );

    let launch_requester = Requester {
        program: "todo".to_owned(),
        id: "todo-launch-context-1".to_owned(),
    };
    let launch = fixture
        .client
        .register_launch_context(&LaunchContextRegistrationV1 {
            version: PROTOCOL_VERSION_V1,
            requester: launch_requester.clone(),
            environment: vec![
                LaunchEnvironmentVariableV1 {
                    name: "CALLER_ONLY".to_owned(),
                    value: "preserved".to_owned(),
                },
                LaunchEnvironmentVariableV1 {
                    name: "SUPER_SECRET".to_owned(),
                    value: "must-not-be-persisted".to_owned(),
                },
                LaunchEnvironmentVariableV1 {
                    name: "CODEX_HOME".to_owned(),
                    value: "/attacker/home".to_owned(),
                },
                LaunchEnvironmentVariableV1 {
                    name: "CODEX_EXEC_SERVER_URL".to_owned(),
                    value: "https://attacker.invalid".to_owned(),
                },
            ],
        })
        .await
        .or_panic("register caller launch context");
    let mut launch_job = fixture.request(
        "job-contract-launch-context",
        launch_requester,
        "CHECK_LAUNCH_CONTEXT",
    );
    launch_job.invocation.workspace_access = WorkspaceAccess::ReadOnly;
    launch_job.invocation.builtin_tools.local_execution = true;
    launch_job.invocation.launch_context = Some(launch.id.clone());
    fixture
        .client
        .submit_job(&launch_job)
        .await
        .or_panic("submit launch-context job");
    wait_for_state(&fixture, &launch_job.id, JobState::Completed).await;
    fixture
        .client
        .submit_job(&launch_job)
        .await
        .or_panic("identical resubmit precedes one-shot context consumption");
    let stored_launch_job = fixture
        .client
        .get_job(&launch_job.id)
        .await
        .or_panic("read launch-context job");
    assert!(
        !serde_json::to_string(&stored_launch_job)
            .or_panic("encode stored launch-context job")
            .contains("must-not-be-persisted")
    );
    let launch_logs = fixture
        .client
        .logs(&launch_job.id, &LogsQueryV1::default())
        .await
        .or_panic("read launch-context logs");
    assert!(
        !serde_json::to_string(&launch_logs)
            .or_panic("encode launch-context logs")
            .contains("must-not-be-persisted")
    );
    let mut reused_context = launch_job.clone();
    reused_context.id = JobId::new("job-contract-consumed-context");
    match fixture.client.submit_job(&reused_context).await {
        Err(ClientError::Api { status, code, .. }) => {
            assert_eq!(status, 404);
            assert_eq!(code, "not_found");
        }
        result => panic!("consumed launch context was accepted: {result:?}"),
    }

    let cancel_request = fixture.request(
        "job-contract-cancel",
        Requester {
            program: "annals".to_owned(),
            id: "annals-contract-7".to_owned(),
        },
        "WAIT_FOR_CANCEL",
    );
    fixture
        .client
        .submit_job(&cancel_request)
        .await
        .or_panic("submit cancellable job");
    wait_for_harness_response(&fixture, &cancel_request.id, 3).await;
    let cancellation = fixture
        .client
        .cancel_job(&cancel_request.id)
        .await
        .or_panic("request cancellation");
    assert!(cancellation.cancellation_requested);
    let cancelled = wait_for_state(&fixture, &cancel_request.id, JobState::Cancelled).await;
    assert_eq!(cancelled.summary.state, JobState::Cancelled);
    let cancelled_logs = fixture
        .client
        .logs(&cancel_request.id, &LogsQueryV1::default())
        .await
        .or_panic("read cancelled job logs");
    let cancellation_events = lifecycle_events(&cancelled_logs.records);
    assert!(cancellation_events.contains(&LifecycleEventKind::CancellationRequested));
    assert!(cancellation_events.contains(&LifecycleEventKind::AttemptCancelled));
    assert!(cancellation_events.contains(&LifecycleEventKind::JobCancelled));

    fixture.shutdown().await;
}

#[tokio::test]
async fn health_degrades_when_authoritative_authentication_is_invalid() {
    let fixture = DaemonFixture::start().await;
    fs::write(
        fixture.temporary.path().join("home/.codex/auth.json"),
        br#"{"tokens":"#,
    )
    .or_panic("truncate authoritative authentication");

    let health = fixture
        .client
        .health()
        .await
        .or_panic("read degraded health");
    assert_eq!(health.status, "degraded");
    assert!(!health.accepting_jobs);
    assert!(!health.authentication.authenticated);
    assert!(
        health
            .authentication
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("auth.json is not valid JSON"))
    );

    fixture.shutdown().await;
}

#[tokio::test]
async fn admission_rejects_unbound_versions_and_incompatible_protocol_schemas() {
    for (version, protocol_schema, expected_detail) in [
        (
            "0.145.0",
            COMPATIBLE_PROTOCOL_SCHEMA,
            "supported version is 0.146.0",
        ),
        (
            SUPPORTED_CODEX_VERSION,
            r#"{"definitions":{}}"#,
            "ClientRequest",
        ),
    ] {
        let fixture = DaemonFixture::start_with(version, protocol_schema).await;
        let health = fixture
            .client
            .health()
            .await
            .or_panic("read incompatible harness health");
        assert_eq!(health.status, "degraded");
        let request = fixture.request(
            &format!("job-rejected-{}", version.replace('.', "-")),
            Requester {
                program: "contract".to_owned(),
                id: format!("request-{version}"),
            },
            "COMPLETE",
        );

        match fixture.client.submit_job(&request).await {
            Err(ClientError::Api {
                status,
                code,
                message,
                ..
            }) => {
                assert_eq!(status, 422);
                assert_eq!(code, "unsupported_setting");
                assert!(
                    message.contains(expected_detail),
                    "unexpected rejection: {message}"
                );
            }
            result => panic!("incompatible harness was admitted: {result:?}"),
        }
        let jobs = fixture
            .client
            .list_jobs(&ListJobsQueryV1::default())
            .await
            .or_panic("list jobs after rejected admission");
        assert!(jobs.jobs.is_empty(), "rejected request reached job storage");

        fixture.shutdown().await;
    }
}

async fn assert_strict_request_rejection(fixture: &DaemonFixture, request: &JobRequestV1) {
    let mut invalid = serde_json::to_value(request).or_panic("encode request");
    invalid
        .pointer_mut("/invocation")
        .and_then(Value::as_object_mut)
        .or_panic("invocation object")
        .insert("providerConfig".to_owned(), json!({ "approval": "never" }));
    let response = fixture
        .raw_client
        .post(format!("{ORIGIN}/v1/jobs"))
        .json(&invalid)
        .send()
        .await
        .or_panic("submit invalid raw request");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let error: ErrorResponseV1 = response.json().await.or_panic("decode API error");
    assert_eq!(error.code, "invalid_json");
    assert!(error.message.contains("providerConfig"));
}

async fn wait_for_harness_response(fixture: &DaemonFixture, job_id: &JobId, id: i64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let job = fixture.client.get_job(job_id).await.or_panic("poll job");
        let logs = fixture
            .client
            .logs(job_id, &LogsQueryV1::default())
            .await
            .or_panic("poll harness logs");
        if logs.records.iter().any(|record| {
            record.stream == LogStream::HarnessOutput
                && serde_json::from_str::<Value>(record.payload.get())
                    .is_ok_and(|payload| payload.get("id").and_then(Value::as_i64) == Some(id))
        }) {
            return;
        }
        assert!(
            !job.summary.state.is_terminal(),
            "job became terminal before harness response {id}: {:?}; {}",
            job.summary.state,
            fixture.diagnostics()
        );
        assert!(
            Instant::now() < deadline,
            "job did not emit harness response {id}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_for_state(
    fixture: &DaemonFixture,
    job_id: &JobId,
    expected: JobState,
) -> nucleus_core::JobV1 {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let job = fixture.client.get_job(job_id).await.or_panic("poll job");
        if job.summary.state == expected {
            return job;
        }
        assert!(
            !job.summary.state.is_terminal(),
            "job reached {:?}, expected {expected:?}: {job:#?}; {}",
            job.summary.state,
            fixture.diagnostics()
        );
        assert!(Instant::now() < deadline, "job did not reach {expected:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn lifecycle_events(records: &[nucleus_core::LogRecordV1]) -> Vec<LifecycleEventKind> {
    records
        .iter()
        .filter(|record| record.stream == LogStream::NucleusLifecycle)
        .map(|record| {
            serde_json::from_str::<LifecycleEventV1>(record.payload.get())
                .or_panic("decode lifecycle record")
                .event
        })
        .collect()
}

fn protocol_request(records: &[nucleus_core::LogRecordV1], method: &str) -> Value {
    records
        .iter()
        .filter(|record| record.stream == LogStream::HarnessInput)
        .filter_map(|record| serde_json::from_str::<Value>(record.payload.get()).ok())
        .find(|message| message.get("method").and_then(Value::as_str) == Some(method))
        .unwrap_or_else(|| panic!("missing {method} protocol request"))
}

fn write_fake_codex(path: &Path, version: &str, protocol_schema: &str) {
    const SCRIPT: &str = r#"#!/bin/sh
set -eu

if [ "${1:-}" = "--version" ]; then
  printf '%s\n' 'codex-cli __CODEX_VERSION__'
  exit 0
fi

if [ "${1:-}" = "debug" ] && [ "${2:-}" = "models" ]; then
  printf '%s\n' '{"models":[{"slug":"fake-model","supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"}],"default_reasoning_level":"low","shell_type":"shell_command","supports_search_tool":false}]}'
  exit 0
fi

case " $* " in
  *" generate-json-schema "*)
    output=''
    take_output=0
    for argument in "$@"; do
      if [ "$take_output" -eq 1 ]; then
        output=$argument
        break
      fi
      if [ "$argument" = "--out" ]; then
        take_output=1
      fi
    done
    test -n "$output"
    mkdir -p "$output"
    printf '%s\n' '__PROTOCOL_SCHEMA__' > "$output/codex_app_server_protocol.schemas.json"
    exit 0
    ;;
esac

IFS= read -r initialize
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{}}'
IFS= read -r initialized
IFS= read -r mcp_list
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"data":[]}}'
IFS= read -r thread_start
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thread-fake"}}}'
IFS= read -r turn_start
printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"turn":{"id":"turn-fake"}}}'

case "$turn_start" in
  *CHECK_LAUNCH_CONTEXT*)
    test "${CALLER_ONLY:-}" = preserved
    test "${SUPER_SECRET:-}" = must-not-be-persisted
    test "${RUST_LOG+x}" != x
    test "${CODEX_EXEC_SERVER_URL+x}" != x
    test "${CODEX_HOME:-}" != /attacker/home
    ;;
  *WAIT_FOR_CANCEL*)
    trap 'exit 0' TERM INT
    while :; do sleep 1; done
    ;;
  *USE_TOOL*)
    printf '%s\n' '{"jsonrpc":"2.0","id":20,"method":"item/tool/call","params":{"threadId":"thread-fake","turnId":"turn-fake","callId":"call-1","namespace":null,"tool":"read_todo","arguments":{"todoId":"todo-1"}}}'
    IFS= read -r tool_response
    case "$tool_response" in
      *'"id":20'*'"success":true'*) ;;
      *) exit 65 ;;
    esac
    ;;
esac

printf '%s\n' '{"jsonrpc":"2.0","method":"item/completed","params":{"threadId":"thread-fake","turnId":"turn-fake","item":{"id":"message-fake","type":"agentMessage","text":"fake completed"}}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"thread-fake","turn":{"id":"turn-fake","status":"completed"}}}'
"#;
    let script = SCRIPT
        .replace("__CODEX_VERSION__", version)
        .replace("__PROTOCOL_SCHEMA__", protocol_schema);
    fs::write(path, script).or_panic("write fake Codex executable");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .or_panic("make fake Codex executable");
}
