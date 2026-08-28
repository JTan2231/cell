use std::collections::BTreeMap;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{
    AbsolutePath, AgentInvocationV1, BuiltinToolsV1, JobId, JobRequestV1, JobState,
    LaunchContextRegistrationV1, LaunchEnvironmentVariableV1, LifecycleEventKind, LifecycleEventV1,
    LogSchemaV1, LogStream, LogsQueryV1, ModelId, PROTOCOL_VERSION_V1, ReasoningEffort, Requester,
    SchemaId, TimeoutSeconds, ToolCallsQueryV1, ToolDefinitionV1, ToolResultV1,
    ToolsetDefinitionsV1, ToolsetRef, ToolsetRegistrationV1, WorkspaceAccess,
};
use serde_json::value::{RawValue, to_raw_value};
use serde_json::{Value, json};
use tokio::runtime::Builder;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::model::ModelQuality;
use crate::tool_server::{self, Backend, Tool, ToolFailure, ToolSuccess};

const DEFAULT_TIMEOUT: Duration = Duration::from_hours(1);
const DEFAULT_STDERR_TAIL_BYTES: usize = 64 * 1024;
const MAILBOX_WAIT_SECONDS: u32 = 1;
const SUMMARY_FALLBACK_POLLS: u32 = 60;
const TOOLSET_DEFINITIONS_SCHEMA_ID: &str = "nucleus.toolset-definitions.v1";
const TOOLSET_NAME: &str = "research-liaison";
const TOOLSET_VERSION: u32 = 1;
const TOOL_INPUT_SCHEMA_ID: &str = "todo.tool.create-todo.input.v1";
const TOOL_RESULT_SCHEMA_ID: &str = "todo.tool.create-todo.result.v1";
const DEVELOPER_INSTRUCTIONS: &str = "Research the direction thoroughly. You may read accessible local and web material, but must not modify anything. Record exactly one todo using only the supplied create_todo tool.";

const TOOL_RESULT_SCHEMA: &str = r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "title":"Todo create_todo result",
  "oneOf":[
    {
      "type":"object",
      "additionalProperties":false,
      "required":["created","todo"],
      "properties":{
        "created":{"const":true},
        "todo":{
          "type":"object",
          "additionalProperties":false,
          "required":["id","title"],
          "properties":{"id":{"type":"string"},"title":{"type":"string"}}
        }
      }
    },
    {
      "type":"object",
      "additionalProperties":false,
      "required":["error"],
      "properties":{
        "error":{
          "type":"object",
          "additionalProperties":false,
          "required":["code","message"],
          "properties":{"code":{"type":"string"},"message":{"type":"string"}}
        }
      }
    }
  ]
}"#;

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ModelSettings {
    model: String,
    reasoning_effort: ReasoningEffort,
}

impl ModelSettings {
    #[must_use]
    pub(crate) fn new(quality: ModelQuality, model: Option<&str>) -> Self {
        let (preset_model, reasoning_effort) = match quality {
            ModelQuality::Low => ("gpt-5.6-luna", ReasoningEffort::Medium),
            ModelQuality::Medium => ("gpt-5.6-terra", ReasoningEffort::Medium),
            ModelQuality::High => ("gpt-5.6-sol", ReasoningEffort::Max),
        };
        Self {
            model: model.unwrap_or(preset_model).to_owned(),
            reasoning_effort,
        }
    }

    #[must_use]
    pub(crate) fn model(&self) -> &str {
        &self.model
    }

    #[must_use]
    pub(crate) const fn reasoning_effort(&self) -> ReasoningEffort {
        self.reasoning_effort
    }
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self::new(ModelQuality::default(), None)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Runner {
    socket: Option<PathBuf>,
    timeout: Duration,
    stderr_tail_bytes: usize,
}

impl Default for Runner {
    fn default() -> Self {
        Self {
            socket: None,
            timeout: DEFAULT_TIMEOUT,
            stderr_tail_bytes: DEFAULT_STDERR_TAIL_BYTES,
        }
    }
}

impl Runner {
    #[must_use]
    pub(crate) fn for_current_user() -> Self {
        Self::default()
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn new(socket: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            socket: Some(socket.into()),
            timeout,
            ..Self::default()
        }
    }

    /// Run one research liaison from the caller's working directory.
    ///
    /// Nucleus owns Codex authentication and execution. Todo retains the prompt, managed tool,
    /// domain transaction, and the rule that a durable creation outranks a later runtime failure.
    pub(crate) fn run_liaison(
        &self,
        settings: &ModelSettings,
        prompt: &str,
        working_directory: &Path,
        backend: &mut impl Backend,
        forward_stderr: bool,
    ) -> AppResult<String> {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                AppError::unexpected(
                    "model_runner_thread",
                    format!("could not initialize the model liaison runtime: {error}"),
                )
            })?;
        let mut diagnostics = Vec::new();
        let execution = runtime.block_on(async {
            tokio::time::timeout(self.timeout, async {
                let client = match &self.socket {
                    Some(socket) => NucleusClient::new(socket),
                    None => NucleusClient::for_current_user(),
                }
                .map_err(|error| runtime_client_error("model_runner_spawn", &error))?;
                self.run_with_client(
                    &client,
                    settings,
                    prompt,
                    working_directory,
                    backend,
                    forward_stderr,
                    &mut diagnostics,
                )
                .await
            })
            .await
        });
        let result = match execution {
            Ok(result) => result,
            Err(_) => Err(timeout_failure()),
        };
        result.map_err(|error| runtime_error(error.code, &error.message, &diagnostics))
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    async fn run_with_client(
        &self,
        client: &NucleusClient,
        settings: &ModelSettings,
        prompt: &str,
        working_directory: &Path,
        backend: &mut impl Backend,
        forward_stderr: bool,
        diagnostics: &mut Vec<u8>,
    ) -> Result<String, RuntimeFailure> {
        let health = client
            .health()
            .await
            .map_err(|error| runtime_client_error("model_runner_spawn", &error))?;
        if health.status != "ok" || !health.accepting_jobs || !health.authentication.authenticated {
            return Err(RuntimeFailure::new(
                "model_runner_failed",
                format!(
                    "Nucleus is not ready: status={}, accepting_jobs={}, authenticated={}",
                    health.status, health.accepting_jobs, health.authentication.authenticated
                ),
            ));
        }

        let registration = toolset_registration()?;
        for schema in tool_schemas()? {
            client
                .register_schema(&schema)
                .await
                .map_err(|error| runtime_client_error("model_runner_tool_schema", &error))?;
        }
        client
            .register_toolset(&registration)
            .await
            .map_err(|error| runtime_client_error("model_runner_tool_schema", &error))?;

        let suffix = Uuid::now_v7().to_string();
        let requester = Requester {
            program: "todo".to_owned(),
            id: format!("todo-request-{suffix}"),
        };
        let launch_context = client
            .register_launch_context(&LaunchContextRegistrationV1 {
                version: PROTOCOL_VERSION_V1,
                requester: requester.clone(),
                environment: launch_environment()?,
            })
            .await
            .map_err(|error| runtime_client_error("model_runner_spawn", &error))?;
        let mut invocation = AgentInvocationV1::new(
            "codex",
            ModelId::new(settings.model()),
            AbsolutePath::new(working_directory),
            WorkspaceAccess::ReadOnly,
            BuiltinToolsV1 {
                local_execution: true,
                web_search: true,
            },
            TimeoutSeconds::new(self.timeout.as_secs().max(1)),
        );
        invocation.reasoning_effort = Some(settings.reasoning_effort());
        invocation.toolset = Some(registration.toolset.clone());
        invocation.launch_context = Some(launch_context.id);
        let mut request = JobRequestV1::new(
            JobId::new(format!("todo-{suffix}")),
            "Todo research liaison",
            requester.clone(),
            tool_server::instructions(),
            prompt,
            invocation,
        );
        request.developer_instructions = Some(DEVELOPER_INSTRUCTIONS.to_owned());

        let accepted = loop {
            match client.submit_job(&request).await {
                Ok(accepted) => break accepted,
                Err(ClientError::Transport { .. }) => {
                    // The daemon may have admitted the stable job before the response was lost.
                    // Resubmitting the identical ID and bytes is the Nucleus idempotency path.
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(error) => {
                    return Err(runtime_client_error("model_runner_failed", &error));
                }
            }
        };
        let job_id = accepted.job_id;
        let mut tool_after = 0;
        // Submission returns as the worker starts. Begin at zero so diagnostics emitted between
        // admission and the HTTP response cannot be skipped.
        let mut log_after = 0;
        let mut todo_created = false;
        let mut final_response = None;
        let mut terminal_state = None;
        let mut terminal_attempt = false;
        let mut polls_since_summary = 0_u32;
        let mut cached_results: BTreeMap<String, CachedToolResult> = BTreeMap::new();

        loop {
            let calls = client
                .pending_tool_calls(
                    &job_id,
                    &ToolCallsQueryV1 {
                        after: tool_after,
                        wait_seconds: MAILBOX_WAIT_SECONDS,
                    },
                )
                .await
                .map_err(|error| runtime_client_error("model_runner_protocol", &error))?;
            for pending in calls.calls {
                let call = pending.call;
                if call.job_id != job_id
                    || call.arguments_schema_id.as_str() != TOOL_INPUT_SCHEMA_ID
                {
                    return Err(RuntimeFailure::new(
                        "model_runner_protocol",
                        "Nucleus returned a tool call outside the admitted Todo contract",
                    ));
                }
                let result = if let Some(result) = cached_results.get(call.id.as_str()) {
                    result.clone()
                } else {
                    let arguments =
                        serde_json::from_str(call.arguments.get()).map_err(|error| {
                            RuntimeFailure::new(
                                "model_runner_protocol",
                                format!("Nucleus returned invalid tool arguments: {error}"),
                            )
                        })?;
                    let result =
                        dispatch_tool_call(backend, &mut todo_created, &call.tool_name, arguments);
                    let (text, success) = model_tool_result(result);
                    let result = CachedToolResult {
                        payload: RawValue::from_string(text).map_err(|error| {
                            RuntimeFailure::new(
                                "model_runner_protocol",
                                format!("could not encode a Todo tool result: {error}"),
                            )
                        })?,
                        is_error: !success,
                    };
                    cached_results.insert(call.id.to_string(), result.clone());
                    result
                };
                let response = ToolResultV1 {
                    version: PROTOCOL_VERSION_V1,
                    call_id: call.id.clone(),
                    requester: requester.clone(),
                    result_schema_id: SchemaId::new(TOOL_RESULT_SCHEMA_ID),
                    result: result.payload,
                    is_error: result.is_error,
                };
                loop {
                    match client.post_tool_result(&job_id, &call.id, &response).await {
                        Ok(_) => break,
                        Err(ClientError::Transport { .. }) => {
                            // The result bytes are cached before this request. An ambiguous reply
                            // is retried byte-for-byte and never re-executes the Todo backend.
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        Err(error) => {
                            return Err(runtime_client_error("model_runner_protocol", &error));
                        }
                    }
                }
                tool_after = tool_after.max(call.request_sequence);
            }

            read_new_logs(
                client,
                &job_id,
                &mut log_after,
                diagnostics,
                forward_stderr,
                self.stderr_tail_bytes,
                &mut final_response,
                &mut terminal_state,
                &mut terminal_attempt,
            )
            .await?;
            polls_since_summary = polls_since_summary.saturating_add(1);
            if terminal_state.is_none()
                && !terminal_attempt
                && polls_since_summary < SUMMARY_FALLBACK_POLLS
            {
                continue;
            }
            let job = client
                .get_job(&job_id)
                .await
                .map_err(|error| runtime_client_error("model_runner_protocol", &error))?;
            if !job.summary.state.is_terminal() {
                terminal_attempt = false;
                polls_since_summary = 0;
                continue;
            }
            read_new_logs(
                client,
                &job_id,
                &mut log_after,
                diagnostics,
                forward_stderr,
                self.stderr_tail_bytes,
                &mut final_response,
                &mut terminal_state,
                &mut terminal_attempt,
            )
            .await?;
            if terminal_state.is_some_and(|state| state != job.summary.state) {
                return Err(RuntimeFailure::new(
                    "model_runner_protocol",
                    "Nucleus job state disagreed with its terminal lifecycle event",
                ));
            }
            return match job.summary.state {
                JobState::Completed => Ok(job
                    .attempts
                    .last()
                    .and_then(|attempt| attempt.output.as_ref())
                    .map_or_else(
                        || final_response.unwrap_or_default(),
                        |output| output.final_message.clone(),
                    )),
                JobState::Failed | JobState::Cancelled => {
                    let attempt = job.attempts.last();
                    let code = if attempt.is_some_and(|attempt| {
                        attempt.state == nucleus_core::AttemptState::TimedOut
                    }) {
                        "model_runner_timeout"
                    } else if attempt.is_some_and(|attempt| {
                        attempt.terminal_reason
                            == Some(nucleus_core::AttemptTerminalReason::ProtocolError)
                    }) {
                        "model_runner_protocol"
                    } else {
                        "model_runner_failed"
                    };
                    let detail = attempt
                        .and_then(|attempt| attempt.terminal_message.as_deref())
                        .unwrap_or("Nucleus ended the liaison without terminal detail");
                    Err(RuntimeFailure::new(code, detail))
                }
                JobState::Accepted | JobState::Running | JobState::WaitingOnRequester => {
                    Err(RuntimeFailure::new(
                        "model_runner_protocol",
                        "Nucleus returned a nonterminal state after reporting terminality",
                    ))
                }
            };
        }
    }
}

fn launch_environment() -> Result<Vec<LaunchEnvironmentVariableV1>, RuntimeFailure> {
    std::env::vars_os()
        .map(|(name, value)| {
            let name = name.into_string().map_err(|_| {
                RuntimeFailure::new(
                    "model_runner_spawn",
                    "the caller environment contains a non-UTF-8 variable name",
                )
            })?;
            let value = value.into_string().map_err(|_| {
                RuntimeFailure::new(
                    "model_runner_spawn",
                    format!("caller environment variable {name:?} contains non-UTF-8 data"),
                )
            })?;
            Ok(LaunchEnvironmentVariableV1 { name, value })
        })
        .collect()
}

#[derive(Clone)]
struct CachedToolResult {
    payload: Box<RawValue>,
    is_error: bool,
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn read_new_logs(
    client: &NucleusClient,
    job_id: &JobId,
    after: &mut u64,
    diagnostics: &mut Vec<u8>,
    forward_stderr: bool,
    stderr_tail_bytes: usize,
    final_response: &mut Option<String>,
    terminal_state: &mut Option<JobState>,
    terminal_attempt: &mut bool,
) -> Result<(), RuntimeFailure> {
    loop {
        let logs = client
            .logs(
                job_id,
                &LogsQueryV1 {
                    after: *after,
                    follow: false,
                    limit: Some(1_000),
                },
            )
            .await
            .map_err(|error| runtime_client_error("model_runner_protocol", &error))?;
        let count = logs.records.len();
        for record in logs.records {
            match record.stream {
                LogStream::HarnessStderr => {
                    let envelope: Value =
                        serde_json::from_str(record.payload.get()).map_err(|error| {
                            RuntimeFailure::new(
                                "model_runner_protocol",
                                format!("Nucleus returned an invalid stderr envelope: {error}"),
                            )
                        })?;
                    if envelope.get("encoding").and_then(Value::as_str) != Some("base64") {
                        return Err(RuntimeFailure::new(
                            "model_runner_protocol",
                            "Nucleus returned an unsupported stderr encoding",
                        ));
                    }
                    let encoded =
                        envelope
                            .get("data")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                RuntimeFailure::new(
                                    "model_runner_protocol",
                                    "Nucleus stderr envelope omitted its data",
                                )
                            })?;
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(encoded)
                        .map_err(|error| {
                            RuntimeFailure::new(
                                "model_runner_protocol",
                                format!("Nucleus returned invalid base64 stderr: {error}"),
                            )
                        })?;
                    retain_stderr(diagnostics, &bytes, stderr_tail_bytes);
                    if forward_stderr {
                        forward_diagnostics(&bytes).map_err(|error| {
                            RuntimeFailure::new(
                                "model_runner_protocol",
                                format!("could not forward model runner diagnostics: {error}"),
                            )
                        })?;
                    }
                }
                LogStream::NucleusLifecycle => {
                    let event: LifecycleEventV1 = serde_json::from_str(record.payload.get())
                        .map_err(|error| {
                            RuntimeFailure::new(
                                "model_runner_protocol",
                                format!("Nucleus returned an invalid lifecycle event: {error}"),
                            )
                        })?;
                    if event.event == LifecycleEventKind::TurnCompleted
                        && let Some(details) = event.details
                    {
                        let details: Value =
                            serde_json::from_str(details.get()).map_err(|error| {
                                RuntimeFailure::new(
                                    "model_runner_protocol",
                                    format!("Nucleus returned invalid turn details: {error}"),
                                )
                            })?;
                        *final_response = details
                            .get("finalMessage")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                    }
                    match event.event {
                        LifecycleEventKind::JobCompleted => {
                            *terminal_state = Some(JobState::Completed);
                        }
                        LifecycleEventKind::JobFailed => {
                            *terminal_state = Some(JobState::Failed);
                        }
                        LifecycleEventKind::JobCancelled => {
                            *terminal_state = Some(JobState::Cancelled);
                        }
                        LifecycleEventKind::AttemptCompleted
                        | LifecycleEventKind::AttemptFailed
                        | LifecycleEventKind::AttemptTimedOut
                        | LifecycleEventKind::AttemptCancelled
                        | LifecycleEventKind::AttemptLost => {
                            // A terminal attempt is enough to consult the durable summary even if
                            // a later job lifecycle record is delayed or unavailable.
                            *terminal_attempt = true;
                        }
                        LifecycleEventKind::JobAccepted
                        | LifecycleEventKind::JobStarted
                        | LifecycleEventKind::AttemptCreated
                        | LifecycleEventKind::HarnessValidated
                        | LifecycleEventKind::ProcessStarted
                        | LifecycleEventKind::ThreadStarted
                        | LifecycleEventKind::TurnStarted
                        | LifecycleEventKind::WaitingOnRequester
                        | LifecycleEventKind::ToolCallPending
                        | LifecycleEventKind::ToolCallAnswered
                        | LifecycleEventKind::CancellationRequested
                        | LifecycleEventKind::TurnCompleted
                        | LifecycleEventKind::ProcessExited
                        | LifecycleEventKind::RecordDecodeFailed => {}
                    }
                }
                LogStream::NucleusControl
                | LogStream::HarnessInput
                | LogStream::HarnessOutput
                | LogStream::Requester => {}
            }
        }
        *after = logs.next_sequence;
        if count < 1_000 {
            return Ok(());
        }
    }
}

fn tool_schemas() -> Result<[LogSchemaV1; 2], RuntimeFailure> {
    let input = tool_server::tool_definitions()
        .into_iter()
        .next()
        .and_then(|definition| definition.get("inputSchema").cloned())
        .ok_or_else(|| {
            RuntimeFailure::new(
                "model_runner_tool_schema",
                "Todo create_todo definition omitted its input schema",
            )
        })?;
    let input = to_raw_value(&input).map_err(|error| {
        RuntimeFailure::new(
            "model_runner_tool_schema",
            format!("could not encode the Todo input schema: {error}"),
        )
    })?;
    let result = RawValue::from_string(TOOL_RESULT_SCHEMA.to_owned()).map_err(|error| {
        RuntimeFailure::new(
            "model_runner_tool_schema",
            format!("the Todo result schema is invalid: {error}"),
        )
    })?;
    Ok([
        LogSchemaV1::new(
            TOOL_INPUT_SCHEMA_ID,
            "Todo create_todo input",
            "1",
            "application/schema+json",
            "todo",
            input,
        ),
        LogSchemaV1::new(
            TOOL_RESULT_SCHEMA_ID,
            "Todo create_todo result",
            "1",
            "application/schema+json",
            "todo",
            result,
        ),
    ])
}

fn toolset_registration() -> Result<ToolsetRegistrationV1, RuntimeFailure> {
    let mut definitions = tool_server::tool_definitions();
    let definition = definitions.pop().ok_or_else(|| {
        RuntimeFailure::new(
            "model_runner_tool_schema",
            "Todo did not define create_todo",
        )
    })?;
    if !definitions.is_empty() {
        return Err(RuntimeFailure::new(
            "model_runner_tool_schema",
            "Todo unexpectedly defined more than one managed tool",
        ));
    }
    let name = required_definition_string(&definition, "name")?;
    let description = required_definition_string(&definition, "description")?;
    let input_schema = definition.get("inputSchema").cloned().ok_or_else(|| {
        RuntimeFailure::new(
            "model_runner_tool_schema",
            "Todo create_todo definition omitted its input schema",
        )
    })?;
    let definitions = ToolsetDefinitionsV1 {
        version: PROTOCOL_VERSION_V1,
        tools: vec![ToolDefinitionV1 {
            name,
            description,
            input_schema_id: SchemaId::new(TOOL_INPUT_SCHEMA_ID),
            input_schema: to_raw_value(&input_schema).map_err(|error| {
                RuntimeFailure::new(
                    "model_runner_tool_schema",
                    format!("could not encode the Todo input schema: {error}"),
                )
            })?,
        }],
    };
    ToolsetRegistrationV1::new(
        ToolsetRef {
            provider: "todo".to_owned(),
            name: TOOLSET_NAME.to_owned(),
            version: TOOLSET_VERSION,
        },
        TOOLSET_DEFINITIONS_SCHEMA_ID,
        definitions,
    )
    .map_err(|error| {
        RuntimeFailure::new(
            "model_runner_tool_schema",
            format!("could not encode the Todo toolset: {error}"),
        )
    })
}

fn required_definition_string(definition: &Value, field: &str) -> Result<String, RuntimeFailure> {
    definition
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            RuntimeFailure::new(
                "model_runner_tool_schema",
                format!("Todo create_todo definition omitted its {field}"),
            )
        })
}

fn dispatch_tool_call(
    backend: &mut impl Backend,
    todo_created: &mut bool,
    name: &str,
    arguments: Value,
) -> Result<ToolSuccess, ToolFailure> {
    let Some(tool) = Tool::from_name(name) else {
        return Err(ToolFailure::new(
            "unknown_tool",
            format!("unknown Todo tool {name:?}"),
        ));
    };
    if *todo_created {
        return Err(ToolFailure::new(
            "todo_already_created",
            "this liaison session has already created its todo",
        ));
    }
    let result = backend.call(tool, arguments);
    if result.as_ref().is_ok_and(ToolSuccess::todo_created) {
        *todo_created = true;
    }
    result
}

fn model_tool_result(result: Result<ToolSuccess, ToolFailure>) -> (String, bool) {
    match result {
        Ok(value) => (
            serde_json::to_string(value.output()).unwrap_or_else(|_| "null".to_owned()),
            true,
        ),
        Err(error) => {
            let value = json!({
                "error": {
                    "code": error.code(),
                    "message": error.message()
                }
            });
            (
                serde_json::to_string(&value).unwrap_or_else(|_| {
                    r#"{"error":{"code":"tool_failed","message":"tool failed"}}"#.to_owned()
                }),
                false,
            )
        }
    }
}

fn retain_stderr(tail: &mut Vec<u8>, chunk: &[u8], limit: usize) {
    if chunk.len() >= limit {
        tail.clear();
        tail.extend_from_slice(&chunk[chunk.len() - limit..]);
        return;
    }
    let excess = tail.len().saturating_add(chunk.len()).saturating_sub(limit);
    if excess > 0 {
        tail.drain(..excess);
    }
    tail.extend_from_slice(chunk);
}

fn forward_diagnostics(bytes: &[u8]) -> io::Result<()> {
    let mut output = io::stderr().lock();
    for byte in bytes {
        if matches!(byte, b'\n' | 0x20..=0x7e | 0x80..=0xff) {
            output.write_all(&[*byte])?;
        } else {
            write!(output, "\\x{byte:02x}")?;
        }
    }
    Ok(())
}

#[derive(Debug)]
struct RuntimeFailure {
    code: &'static str,
    message: String,
}

impl RuntimeFailure {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

fn timeout_failure() -> RuntimeFailure {
    RuntimeFailure::new(
        "model_runner_timeout",
        "model liaison exceeded its time limit",
    )
}

fn runtime_client_error(code: &'static str, error: &ClientError) -> RuntimeFailure {
    RuntimeFailure::new(code, format!("Nucleus request failed: {error}"))
}

fn runtime_error(code: &'static str, message: &str, diagnostics: &[u8]) -> AppError {
    let diagnostic = String::from_utf8_lossy(diagnostics);
    let diagnostic = diagnostic.trim();
    let suffix = if diagnostic.is_empty() {
        String::new()
    } else {
        format!(
            "; stderr: {}",
            diagnostic
                .chars()
                .flat_map(char::escape_default)
                .collect::<String>()
        )
    };
    AppError::unexpected(code, format!("{message}{suffix}"))
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::thread;
    use std::time::Duration;

    use serde_json::{Value, json};

    use crate::model::ModelQuality;
    use crate::tool_server::{Backend, Tool, ToolFailure, ToolSuccess};

    use super::{
        DEVELOPER_INSTRUCTIONS, ModelSettings, Runner, TOOL_INPUT_SCHEMA_ID, TOOL_RESULT_SCHEMA_ID,
        dispatch_tool_call, model_tool_result, retain_stderr, tool_schemas, toolset_registration,
    };

    #[test]
    fn quality_presets_match_annals() {
        for (quality, model, effort) in [
            (ModelQuality::Low, "gpt-5.6-luna", "medium"),
            (ModelQuality::Medium, "gpt-5.6-terra", "medium"),
            (ModelQuality::High, "gpt-5.6-sol", "max"),
        ] {
            let settings = ModelSettings::new(quality, None);
            assert_eq!(settings.model(), model);
            assert_eq!(
                serde_json::to_value(settings.reasoning_effort())
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .as_deref(),
                Some(effort)
            );
        }
    }

    #[test]
    fn nucleus_inventory_contains_only_managed_creation() {
        let Ok(registration) = toolset_registration() else {
            panic!("valid Todo toolset was rejected");
        };
        assert_eq!(registration.toolset.provider, "todo");
        assert_eq!(registration.toolset.name, "research-liaison");
        assert_eq!(registration.definitions.tools.len(), 1);
        let tool = &registration.definitions.tools[0];
        assert_eq!(tool.name, "create_todo");
        assert_eq!(tool.input_schema_id.as_str(), TOOL_INPUT_SCHEMA_ID);
        let Ok(schemas) = tool_schemas() else {
            panic!("valid Todo schemas were rejected");
        };
        assert_eq!(schemas[0].id.as_str(), TOOL_INPUT_SCHEMA_ID);
        assert_eq!(schemas[1].id.as_str(), TOOL_RESULT_SCHEMA_ID);
    }

    #[derive(Default)]
    struct StubBackend {
        calls: Vec<Value>,
        reject_next: bool,
    }

    impl Backend for StubBackend {
        fn call(&mut self, tool: Tool, arguments: Value) -> Result<ToolSuccess, ToolFailure> {
            assert_eq!(tool, Tool::CreateTodo);
            self.calls.push(arguments);
            if self.reject_next {
                self.reject_next = false;
                Err(ToolFailure::new("invalid_todo", "the title is blank"))
            } else {
                Ok(ToolSuccess::created(json!({
                    "created": true,
                    "todo": { "id": "t1", "title": "Actionable title" }
                })))
            }
        }
    }

    #[test]
    fn rejected_creation_can_retry_but_success_cannot_duplicate() {
        let mut backend = StubBackend {
            calls: Vec::new(),
            reject_next: true,
        };
        let mut created = false;
        assert!(
            dispatch_tool_call(
                &mut backend,
                &mut created,
                "create_todo",
                json!({ "title": "", "note": "note" }),
            )
            .is_err()
        );
        assert!(!created);
        assert!(
            dispatch_tool_call(
                &mut backend,
                &mut created,
                "create_todo",
                json!({ "title": "Title", "note": "Note" }),
            )
            .is_ok()
        );
        assert!(created);
        let duplicate = dispatch_tool_call(
            &mut backend,
            &mut created,
            "create_todo",
            json!({ "title": "Second", "note": "Second" }),
        );
        assert_eq!(
            duplicate.as_ref().err().map(ToolFailure::code),
            Some("todo_already_created")
        );
        assert_eq!(backend.calls.len(), 2);
    }

    #[test]
    fn tool_results_keep_existing_model_envelope() {
        let (success, success_flag) = model_tool_result(Ok(ToolSuccess::created(json!({
            "created": true,
            "todo": { "id": "t1", "title": "Title" }
        }))));
        assert!(success_flag);
        let Ok(success) = serde_json::from_str::<Value>(&success) else {
            panic!("success envelope was not JSON");
        };
        assert_eq!(
            success,
            json!({ "created": true, "todo": { "id": "t1", "title": "Title" } })
        );

        let (failure, success_flag) =
            model_tool_result(Err(ToolFailure::new("invalid_title", "blank")));
        assert!(!success_flag);
        let Ok(failure) = serde_json::from_str::<Value>(&failure) else {
            panic!("failure envelope was not JSON");
        };
        assert_eq!(
            failure,
            json!({ "error": { "code": "invalid_title", "message": "blank" } })
        );
    }

    #[test]
    fn stderr_tail_is_bounded() {
        let mut tail = b"1234".to_vec();
        retain_stderr(&mut tail, b"56789", 6);
        assert_eq!(tail, b"456789");
        retain_stderr(&mut tail, b"abcdefgh", 6);
        assert_eq!(tail, b"cdefgh");
    }

    #[test]
    fn nucleus_job_preserves_policy_and_dispatches_creation()
    -> Result<(), Box<dyn std::error::Error>> {
        exercise_fake_nucleus(true)
    }

    #[test]
    fn terminal_attempt_event_falls_back_to_durable_job_summary()
    -> Result<(), Box<dyn std::error::Error>> {
        exercise_fake_nucleus(false)
    }

    fn exercise_fake_nucleus(include_job_terminal: bool) -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let socket = directory.path().join("nucleus.sock");
        let listener = UnixListener::bind(&socket)?;
        let working = directory.path().join("research-root");
        std::fs::create_dir(&working)?;
        let working = std::fs::canonicalize(working)?;
        let expected_working = working.display().to_string();
        let server = thread::spawn(move || {
            serve_fake_nucleus(&listener, &expected_working, include_job_terminal)
        });

        let settings = ModelSettings::new(ModelQuality::Low, Some("custom-model"));
        let runner = Runner::new(&socket, Duration::from_secs(5));
        let mut backend = StubBackend::default();
        let diagnostic = runner.run_liaison(
            &settings,
            "Research this need",
            &working,
            &mut backend,
            false,
        )?;
        assert_eq!(diagnostic, "created");
        assert_eq!(backend.calls.len(), 1);
        assert_eq!(backend.calls[0]["title"], "Actionable title");

        let Ok(server_result) = server.join() else {
            panic!("fake Nucleus server panicked");
        };
        if let Err(error) = server_result {
            return Err(std::io::Error::other(error.to_string()).into());
        }
        Ok(())
    }

    fn serve_fake_nucleus(
        listener: &UnixListener,
        expected_working: &str,
        include_job_terminal: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut submitted = None;
        let mut job_id = None;
        for step in 0..11 {
            let (mut stream, _) = listener.accept()?;
            let (request_line, body) = read_request(&stream)?;
            let response = fake_response(
                step,
                &request_line,
                body.as_deref(),
                expected_working,
                include_job_terminal,
                &mut submitted,
                &mut job_id,
            );
            write_response(&mut stream, &response)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn fake_response(
        step: usize,
        request_line: &str,
        body: Option<&str>,
        expected_working: &str,
        include_job_terminal: bool,
        submitted: &mut Option<Value>,
        job_id: &mut Option<String>,
    ) -> Value {
        match step {
            0 => {
                assert!(request_line.starts_with("GET /v1/health "));
                json!({
                    "version": 1,
                    "status": "ok",
                    "daemonVersion": "test",
                    "acceptingJobs": true,
                    "checkedAt": "2026-08-27T00:00:00Z",
                    "supportedProtocolVersions": [1],
                    "authentication": {
                        "codexHome": "/tmp/codex-home",
                        "configured": true,
                        "authenticated": true
                    }
                })
            }
            1 | 2 => {
                assert!(request_line.starts_with("POST /v1/schemas "));
                decode_body(body)
            }
            3 => {
                assert!(request_line.starts_with("POST /v1/toolsets "));
                let registration = decode_body(body);
                assert_eq!(registration["toolset"]["provider"], "todo");
                assert_eq!(
                    registration["definitions"]["tools"][0]["name"],
                    "create_todo"
                );
                json!({
                    "version": 1,
                    "toolset": registration["toolset"],
                    "definitionsSchemaId": registration["definitionsSchemaId"],
                    "digest": registration["digest"],
                    "registeredAt": "2026-08-27T00:00:00Z"
                })
            }
            4 => {
                assert!(request_line.starts_with("POST /v1/launch-contexts "));
                let context = decode_body(body);
                assert_eq!(context["requester"]["program"], "todo");
                assert!(
                    context["environment"]
                        .as_array()
                        .is_some_and(|env| !env.is_empty())
                );
                json!({
                    "version": 1,
                    "id": "todo-launch-context",
                    "expiresAt": "2026-08-27T00:01:00Z"
                })
            }
            5 => {
                assert!(request_line.starts_with("POST /v1/jobs "));
                let request = decode_body(body);
                assert_eq!(request["developerInstructions"], DEVELOPER_INSTRUCTIONS);
                assert_eq!(request["instructions"], super::tool_server::instructions());
                assert_eq!(request["prompt"], "Research this need");
                assert_eq!(request["invocation"]["model"], "custom-model");
                assert_eq!(request["invocation"]["reasoningEffort"], "medium");
                assert_eq!(request["invocation"]["cwd"], expected_working);
                assert_eq!(request["invocation"]["workspaceAccess"], "read-only");
                assert_eq!(
                    request["invocation"]["builtinTools"]["localExecution"],
                    true
                );
                assert_eq!(request["invocation"]["builtinTools"]["webSearch"], true);
                assert_eq!(
                    request["invocation"]["launchContext"],
                    "todo-launch-context"
                );
                let id = request["id"].as_str().map(str::to_owned);
                assert!(id.is_some());
                id.clone_into(job_id);
                *submitted = Some(request.clone());
                json!({
                    "version": 1,
                    "jobId": id,
                    "state": "accepted",
                    "requestDigest": "sha256:fake",
                    "logCursor": 0
                })
            }
            6 => {
                assert!(request_line.contains("/tool-calls?"));
                let id = required_test_value(job_id.as_ref());
                json!({
                    "version": 1,
                    "jobId": id,
                    "calls": [{
                        "version": 1,
                        "call": {
                            "version": 1,
                            "id": "call-1",
                            "jobId": id,
                            "attemptId": "attempt-1",
                            "requestSequence": 1,
                            "toolName": "create_todo",
                            "argumentsSchemaId": TOOL_INPUT_SCHEMA_ID,
                            "arguments": {
                                "title": "Actionable title",
                                "note": "Researched note"
                            }
                        },
                        "state": "pending",
                        "createdAt": "2026-08-27T00:00:00Z"
                    }],
                    "nextSequence": 1
                })
            }
            7 => {
                assert!(request_line.contains("/tool-calls/call-1/result "));
                let result = decode_body(body);
                assert_eq!(result["requester"]["program"], "todo");
                assert_eq!(result["resultSchemaId"], TOOL_RESULT_SCHEMA_ID);
                assert_eq!(result["isError"], false);
                assert_eq!(result["result"]["created"], true);
                assert_eq!(result["result"]["todo"]["id"], "t1");
                let id = required_test_value(job_id.as_ref());
                json!({
                    "version": 1,
                    "call": {
                        "version": 1,
                        "id": "call-1",
                        "jobId": id,
                        "attemptId": "attempt-1",
                        "requestSequence": 1,
                        "toolName": "create_todo",
                        "argumentsSchemaId": TOOL_INPUT_SCHEMA_ID,
                        "arguments": {
                            "title": "Actionable title",
                            "note": "Researched note"
                        }
                    },
                    "state": "answered",
                    "createdAt": "2026-08-27T00:00:00Z",
                    "answeredAt": "2026-08-27T00:00:01Z",
                    "resultSequence": 2
                })
            }
            8 => {
                assert!(request_line.contains("/logs?"));
                let id = required_test_value(job_id.as_ref());
                let mut response = json!({
                    "version": 1,
                    "jobId": id,
                    "records": [{
                        "version": 1,
                        "jobId": id,
                        "attemptId": "attempt-1",
                        "sequence": 1,
                        "observedAt": "2026-08-27T00:00:01Z",
                        "stream": "harness.stderr",
                        "schemaId": "nucleus.raw-bytes.v1",
                        "payload": {"encoding":"base64","data":"ZGlhZ25vc3RpYwo="},
                        "payloadDigest": "sha256:fake"
                    }, {
                        "version": 1,
                        "jobId": id,
                        "attemptId": "attempt-1",
                        "sequence": 2,
                        "observedAt": "2026-08-27T00:00:01Z",
                        "stream": "nucleus.lifecycle",
                        "schemaId": "nucleus.lifecycle-event.v1",
                        "payload": {
                            "version": 1,
                            "event": "turn_completed",
                            "jobId": id,
                            "attemptId": "attempt-1",
                            "details": {
                                "threadId": "thread-1",
                                "turnId": "turn-1",
                                "finalMessage": "lifecycle fallback"
                            }
                        },
                        "payloadDigest": "sha256:fake"
                    }, {
                        "version": 1,
                        "jobId": id,
                        "attemptId": "attempt-1",
                        "sequence": 3,
                        "observedAt": "2026-08-27T00:00:01Z",
                        "stream": "nucleus.lifecycle",
                        "schemaId": "nucleus.lifecycle-event.v1",
                        "payload": {
                            "version": 1,
                            "event": "attempt_completed",
                            "jobId": id,
                            "attemptId": "attempt-1",
                            "message": "Codex turn completed"
                        },
                        "payloadDigest": "sha256:fake"
                    }, {
                        "version": 1,
                        "jobId": id,
                        "attemptId": "attempt-1",
                        "sequence": 4,
                        "observedAt": "2026-08-27T00:00:01Z",
                        "stream": "nucleus.lifecycle",
                        "schemaId": "nucleus.lifecycle-event.v1",
                        "payload": {
                            "version": 1,
                            "event": "job_completed",
                            "jobId": id,
                            "attemptId": "attempt-1",
                            "message": "Codex turn completed"
                        },
                        "payloadDigest": "sha256:fake"
                    }],
                    "nextSequence": 4
                });
                if !include_job_terminal {
                    let Some(records) = response["records"].as_array_mut() else {
                        panic!("fake log records were not an array");
                    };
                    records.pop();
                    response["nextSequence"] = json!(3);
                }
                response
            }
            9 => {
                assert!(request_line.starts_with("GET /v1/jobs/"));
                assert!(!request_line.contains("/logs?"));
                let id = required_test_value(job_id.as_ref());
                let request = required_test_value(submitted.as_ref());
                json!({
                    "version": 1,
                    "summary": {
                        "version": 1,
                        "id": id,
                        "label": "Todo research liaison",
                        "requester": request["requester"],
                        "state": "completed",
                        "requestDigest": "sha256:fake",
                        "createdAt": "2026-08-27T00:00:00Z",
                        "updatedAt": "2026-08-27T00:00:01Z",
                        "completedAt": "2026-08-27T00:00:01Z",
                        "currentAttemptId": "attempt-1"
                    },
                    "request": request,
                    "attempts": [{
                        "version": 1,
                        "id": "attempt-1",
                        "jobId": id,
                        "ordinal": 1,
                        "harness": {
                            "harness": "codex",
                            "harnessVersion": "0.146.0",
                            "adapterVersion": "test"
                        },
                        "state": "completed",
                        "createdAt": "2026-08-27T00:00:00Z",
                        "startedAt": "2026-08-27T00:00:00Z",
                        "completedAt": "2026-08-27T00:00:01Z",
                        "terminalReason": "completed",
                        "terminalMessage": "Codex turn completed",
                        "output": {
                            "threadId": "thread-1",
                            "turnId": "turn-1",
                            "finalMessage": "created"
                        }
                    }]
                })
            }
            10 => {
                assert!(request_line.contains("/logs?"));
                json!({
                    "version": 1,
                    "jobId": required_test_value(job_id.as_ref()),
                    "records": [],
                    "nextSequence": if include_job_terminal { 4 } else { 3 }
                })
            }
            _ => panic!("unexpected fake Nucleus request"),
        }
    }

    fn decode_body(body: Option<&str>) -> Value {
        let Some(body) = body else {
            panic!("request omitted body");
        };
        let Ok(value) = serde_json::from_str(body) else {
            panic!("request body was not JSON");
        };
        value
    }

    fn required_test_value<T>(value: Option<&T>) -> &T {
        let Some(value) = value else {
            panic!("fake server state was absent");
        };
        value
    }

    fn read_request(stream: &UnixStream) -> std::io::Result<(String, Option<String>)> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut request_line = String::new();
        reader.read_line(&mut request_line)?;
        let mut content_length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line)?;
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line
                .strip_prefix("content-length:")
                .or_else(|| line.strip_prefix("Content-Length:"))
            {
                content_length = value
                    .trim()
                    .parse()
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            }
        }
        let body = if content_length == 0 {
            None
        } else {
            let mut bytes = vec![0; content_length];
            reader.read_exact(&mut bytes)?;
            Some(
                String::from_utf8(bytes)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
            )
        };
        Ok((request_line, body))
    }

    fn write_response(stream: &mut UnixStream, response: &Value) -> std::io::Result<()> {
        let body = serde_json::to_vec(response).map_err(std::io::Error::other)?;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )?;
        stream.write_all(&body)?;
        stream.flush()
    }
}
