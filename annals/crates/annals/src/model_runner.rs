use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{
    AbsolutePath, AgentInvocationV1, BuiltinToolsV1, JobId, JobRequestV1, JobState, LogSchemaV1,
    PROTOCOL_VERSION_V1, ReasoningEffort, Requester, SchemaId, TimeoutSeconds, ToolDefinitionV1,
    ToolResultV1, ToolsetDefinitionsV1, ToolsetRef, ToolsetRegistrationV1, WorkspaceAccess,
};
use serde::Deserialize;
use serde_json::value::{RawValue, to_raw_value};
use serde_json::{Value, json};

use crate::error::{AppError, AppResult};
use crate::tool_server::{self, Backend, Tool, ToolFailure, ToolSuccess};

const DEFAULT_TIMEOUT: Duration = Duration::from_hours(1);
const AUTH_PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(250);
const TERMINAL_STATE_PROBE_INTERVAL: Duration = Duration::from_secs(1);
const BEST_EFFORT_CANCEL_TIMEOUT: Duration = Duration::from_millis(250);
const TOOLSET_DEFINITIONS_SCHEMA: &str = "nucleus.toolset-definitions.v1";
const TOOL_RESULT_SCHEMA: &str = "annals.liaison-tool-result.v1";
const TOOLSET_NAME: &str = "liaison";
const TOOLSET_VERSION: u32 = 1;
const DEVELOPER_INSTRUCTIONS: &str = "Use only the nine supplied Annals tools. Complete the session by recording exactly one reconciliation. A successful partial submit or revision is not terminal; correct only the named operations until a tool reports recorded true.";
const TOOL_RESULT_SCHEMA_DOCUMENT: &str = r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "title":"Annals liaison tool result v1",
  "type":"object"
}"#;

#[derive(Debug, Clone, Copy, Default, Deserialize, Eq, PartialEq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ModelQuality {
    Low,
    Medium,
    #[default]
    High,
}

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
    pub(crate) const fn reasoning_effort(&self) -> &'static str {
        match self.reasoning_effort {
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
            ReasoningEffort::Max => "max",
        }
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
}

impl Default for Runner {
    fn default() -> Self {
        Self {
            socket: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl Runner {
    #[must_use]
    pub(crate) fn for_socket(socket: Option<&Path>) -> Self {
        Self {
            socket: socket.map(Path::to_path_buf),
            ..Self::default()
        }
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn new(socket: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            socket: Some(socket.into()),
            timeout,
        }
    }

    /// Verify that Nucleus can use its owned Codex authentication without starting a turn.
    pub(crate) fn preflight_auth(&self) -> AppResult<()> {
        let runtime = runtime()?;
        let client = self.client()?;
        let result = runtime.block_on(async {
            tokio::time::timeout(
                AUTH_PREFLIGHT_TIMEOUT.min(self.timeout),
                client.account_preflight(),
            )
            .await
        });
        match result {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(error)) => Err(auth_error(&error.to_string())),
            Err(_) => Err(auth_error(
                "Nucleus authentication preflight exceeded its time limit",
            )),
        }
    }

    /// Best-effort cleanup for a model run whose domain recovery is already authoritative.
    pub(crate) fn cancel_liaison(&self, model_run_token: &str) {
        let (Ok(runtime), Ok(client)) = (runtime(), self.client()) else {
            return;
        };
        let job_id = JobId::new(format!("annals-{model_run_token}"));
        runtime.block_on(cancel_job_bounded(&client, &job_id));
    }

    /// Run one Nucleus-owned Codex liaison whose only managed tools are Annals' nine tools.
    ///
    /// The returned final response is diagnostic only. Application success is determined by the
    /// reconciliation side effect recorded through `submit_reconciliation`.
    pub(crate) fn run_liaison(
        &self,
        settings: &ModelSettings,
        prompt: &str,
        model_run_token: &str,
        backend: &mut impl Backend,
        forward_stderr: bool,
    ) -> AppResult<String> {
        self.run_liaison_cancellable(
            settings,
            prompt,
            model_run_token,
            backend,
            forward_stderr,
            &|| false,
        )
    }

    pub(crate) fn run_liaison_cancellable(
        &self,
        settings: &ModelSettings,
        prompt: &str,
        model_run_token: &str,
        backend: &mut impl Backend,
        _forward_stderr: bool,
        cancellation_requested: &dyn Fn() -> bool,
    ) -> AppResult<String> {
        if cancellation_requested() {
            return Err(interrupted_error());
        }
        let deadline = Instant::now() + self.timeout;
        let runtime = runtime()?;
        let client = self.client()?;
        runtime.block_on(self.run_job(
            &client,
            settings,
            prompt,
            model_run_token,
            backend,
            cancellation_requested,
            deadline,
        ))
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    async fn run_job(
        &self,
        client: &NucleusClient,
        settings: &ModelSettings,
        prompt: &str,
        model_run_token: &str,
        backend: &mut impl Backend,
        cancellation_requested: &dyn Fn() -> bool,
        deadline: Instant,
    ) -> AppResult<String> {
        register_runtime_contract(client, deadline, cancellation_requested).await?;
        ensure_before_deadline(deadline)?;

        let job_id = JobId::new(format!("annals-{model_run_token}"));
        let requester = Requester {
            program: "annals".to_owned(),
            id: model_run_token.to_owned(),
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout_seconds = remaining.as_secs().max(1);
        let mut invocation = AgentInvocationV1::new(
            "codex",
            settings.model(),
            AbsolutePath::new(std::env::temp_dir()),
            WorkspaceAccess::None,
            BuiltinToolsV1 {
                local_execution: false,
                web_search: false,
            },
            TimeoutSeconds::new(timeout_seconds),
        );
        invocation.reasoning_effort = Some(settings.reasoning_effort);
        invocation.toolset = Some(toolset_ref());
        let mut request = JobRequestV1::new(
            job_id.clone(),
            format!("Annals examination {model_run_token}"),
            requester.clone(),
            tool_server::instructions(),
            prompt,
            invocation,
        );
        request.developer_instructions = Some(DEVELOPER_INSTRUCTIONS.to_owned());
        loop {
            match await_client_call(
                client,
                Some(&job_id),
                client.submit_job(&request),
                deadline,
                cancellation_requested,
            )
            .await
            {
                Ok(_) => break,
                Err(ClientCallError::Client(ClientError::Transport { .. })) => {
                    tokio::time::sleep(CANCELLATION_POLL_INTERVAL).await;
                }
                Err(error) => return Err(client_call_error(error, "model_runner_spawn")),
            }
        }

        let mut call_cursor = 0_u64;
        let mut reconciliation_recorded = false;
        let mut cached_results = BTreeMap::<String, CachedResult>::new();
        let mut last_terminal_probe = None;

        loop {
            if cancellation_requested() {
                cancel_job_bounded(client, &job_id).await;
                return Err(interrupted_error());
            }
            if Instant::now() >= deadline {
                cancel_job_bounded(client, &job_id).await;
                return Err(timeout_error());
            }

            let query = nucleus_core::ToolCallsQueryV1 {
                after: call_cursor,
                wait_seconds: 0,
            };
            let calls = await_retryable_read(
                client,
                &job_id,
                || client.pending_tool_calls(&job_id, &query),
                deadline,
                cancellation_requested,
            )
            .await
            .map_err(|error| client_call_error(error, "model_runner_failed"))?;
            if calls.job_id != job_id {
                return Err(runtime_error(
                    "model_runner_protocol",
                    "Nucleus returned a managed-tool mailbox for a different job",
                ));
            }
            for pending in calls.calls {
                let call = pending.call;
                if !tool_call_matches_contract(
                    &job_id,
                    &call.job_id,
                    &call.tool_name,
                    &call.arguments_schema_id,
                ) {
                    return Err(runtime_error(
                        "model_runner_protocol",
                        "Nucleus returned a tool call outside the admitted Annals contract",
                    ));
                }
                let cached = if let Some(cached) = cached_results.get(call.id.as_str()) {
                    cached.clone()
                } else {
                    let arguments =
                        serde_json::from_str::<Value>(call.arguments.get()).map_err(|error| {
                            runtime_error(
                                "model_runner_protocol",
                                &format!(
                                    "Nucleus returned invalid managed-tool arguments: {error}"
                                ),
                            )
                        })?;
                    let result = if arguments.is_object() {
                        dispatch_tool_call(
                            backend,
                            &mut reconciliation_recorded,
                            &call.tool_name,
                            arguments,
                        )
                    } else {
                        Err(ToolFailure::new(
                            "invalid_tool_call",
                            "Annals tool calls require a known tool name and object arguments",
                        ))
                    };
                    let (text, success) = model_tool_result(result);
                    let cached = CachedResult {
                        value: RawValue::from_string(text).map_err(|error| {
                            AppError::unexpected(
                                "model_runner_protocol",
                                format!("could not encode an Annals tool result: {error}"),
                            )
                        })?,
                        is_error: !success,
                    };
                    cached_results.insert(call.id.to_string(), cached.clone());
                    cached
                };
                let result = ToolResultV1 {
                    version: PROTOCOL_VERSION_V1,
                    call_id: call.id.clone(),
                    requester: requester.clone(),
                    result_schema_id: SchemaId::new(TOOL_RESULT_SCHEMA),
                    result: cached.value,
                    is_error: cached.is_error,
                };
                loop {
                    match await_client_call(
                        client,
                        Some(&job_id),
                        client.post_tool_result(&job_id, &call.id, &result),
                        deadline,
                        cancellation_requested,
                    )
                    .await
                    {
                        Ok(_) => break,
                        Err(ClientCallError::Client(ClientError::Transport { .. })) => {
                            tokio::time::sleep(CANCELLATION_POLL_INTERVAL).await;
                        }
                        Err(error) => {
                            return Err(client_call_error(error, "model_runner_failed"));
                        }
                    }
                }
                call_cursor = call.request_sequence;
            }

            let terminal_job = if last_terminal_probe
                .is_none_or(|probe: Instant| probe.elapsed() >= TERMINAL_STATE_PROBE_INTERVAL)
            {
                last_terminal_probe = Some(Instant::now());
                let job = await_retryable_read(
                    client,
                    &job_id,
                    || client.get_job(&job_id),
                    deadline,
                    cancellation_requested,
                )
                .await
                .map_err(|error| client_call_error(error, "model_runner_failed"))?;
                if job.summary.state.is_terminal() {
                    Some(job)
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(job) = terminal_job {
                if job.summary.state == JobState::Completed {
                    if let Some(output) = job
                        .attempts
                        .last()
                        .and_then(|attempt| attempt.output.as_ref())
                    {
                        return Ok(output.final_message.clone());
                    }
                    return read_final_response(client, &job_id, deadline, cancellation_requested)
                        .await;
                }
                let attempt = job.attempts.last();
                let reason = attempt.and_then(|attempt| attempt.terminal_reason);
                let message = attempt
                    .and_then(|attempt| attempt.terminal_message.as_deref())
                    .unwrap_or("Nucleus ended the model liaison without a completion detail");
                let code = match reason {
                    Some(nucleus_core::AttemptTerminalReason::TimedOut) => "model_runner_timeout",
                    Some(nucleus_core::AttemptTerminalReason::ProtocolError) => {
                        "model_runner_protocol"
                    }
                    Some(nucleus_core::AttemptTerminalReason::Cancelled)
                        if cancellation_requested() =>
                    {
                        "model_runner_interrupted"
                    }
                    _ => "model_runner_failed",
                };
                return Err(runtime_error(code, message));
            }
            tokio::time::sleep(CANCELLATION_POLL_INTERVAL).await;
        }
    }

    fn client(&self) -> AppResult<NucleusClient> {
        let result = self
            .socket
            .as_ref()
            .map_or_else(NucleusClient::for_current_user, |socket| {
                NucleusClient::new(socket.clone())
            });
        result.map_err(|error| client_error("model_runner_spawn", &error))
    }
}

#[derive(Clone)]
struct CachedResult {
    value: Box<RawValue>,
    is_error: bool,
}

fn runtime() -> AppResult<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            AppError::unexpected(
                "model_runner_thread",
                format!("could not start the Nucleus client runtime: {error}"),
            )
        })
}

enum ClientCallError {
    Client(ClientError),
    Interrupted,
    TimedOut,
}

async fn await_client_call<T>(
    client: &NucleusClient,
    job_id: Option<&JobId>,
    future: impl Future<Output = Result<T, ClientError>>,
    deadline: Instant,
    cancellation_requested: &dyn Fn() -> bool,
) -> Result<T, ClientCallError> {
    tokio::pin!(future);
    loop {
        if cancellation_requested() {
            if let Some(job_id) = job_id {
                cancel_job_bounded(client, job_id).await;
            }
            return Err(ClientCallError::Interrupted);
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            if let Some(job_id) = job_id {
                cancel_job_bounded(client, job_id).await;
            }
            return Err(ClientCallError::TimedOut);
        };
        let wait = remaining.min(CANCELLATION_POLL_INTERVAL);
        if let Ok(result) = tokio::time::timeout(wait, &mut future).await {
            return result.map_err(ClientCallError::Client);
        }
    }
}

async fn await_retryable_read<T, Call, CallFuture>(
    client: &NucleusClient,
    job_id: &JobId,
    call: Call,
    deadline: Instant,
    cancellation_requested: &dyn Fn() -> bool,
) -> Result<T, ClientCallError>
where
    Call: Fn() -> CallFuture,
    CallFuture: Future<Output = Result<T, ClientError>>,
{
    loop {
        match await_client_call(
            client,
            Some(job_id),
            call(),
            deadline,
            cancellation_requested,
        )
        .await
        {
            Err(ClientCallError::Client(ClientError::Transport { .. })) => {
                tokio::time::sleep(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(CANCELLATION_POLL_INTERVAL),
                )
                .await;
            }
            result => return result,
        }
    }
}

async fn cancel_job_bounded(client: &NucleusClient, job_id: &JobId) {
    let _ = tokio::time::timeout(BEST_EFFORT_CANCEL_TIMEOUT, client.cancel_job(job_id)).await;
}

fn client_call_error(error: ClientCallError, code: &'static str) -> AppError {
    match error {
        ClientCallError::Client(error) => client_error(code, &error),
        ClientCallError::Interrupted => interrupted_error(),
        ClientCallError::TimedOut => timeout_error(),
    }
}

async fn register_runtime_contract(
    client: &NucleusClient,
    deadline: Instant,
    cancellation_requested: &dyn Fn() -> bool,
) -> AppResult<()> {
    let result_schema = LogSchemaV1::new(
        TOOL_RESULT_SCHEMA,
        "Annals liaison tool result",
        "1",
        "application/schema+json",
        "annals",
        RawValue::from_string(TOOL_RESULT_SCHEMA_DOCUMENT.to_owned()).map_err(|error| {
            AppError::unexpected(
                "model_runner_tool_schema",
                format!("invalid built-in Annals result schema: {error}"),
            )
        })?,
    );
    await_client_call(
        client,
        None,
        client.register_schema(&result_schema),
        deadline,
        cancellation_requested,
    )
    .await
    .map_err(|error| client_call_error(error, "model_runner_tool_schema"))?;
    let toolset = toolset_registration()?;
    await_client_call(
        client,
        None,
        client.register_toolset(&toolset),
        deadline,
        cancellation_requested,
    )
    .await
    .map_err(|error| client_call_error(error, "model_runner_tool_schema"))?;
    Ok(())
}

fn toolset_registration() -> AppResult<ToolsetRegistrationV1> {
    let tools = tool_server::tool_definitions()
        .into_iter()
        .map(|definition| {
            let name = definition
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::unexpected(
                        "model_runner_tool_schema",
                        "an Annals tool definition omitted its name",
                    )
                })?;
            let description = definition
                .get("description")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::unexpected(
                        "model_runner_tool_schema",
                        format!("Annals tool {name:?} omitted its description"),
                    )
                })?;
            let input_schema = definition.get("inputSchema").ok_or_else(|| {
                AppError::unexpected(
                    "model_runner_tool_schema",
                    format!("Annals tool {name:?} omitted its input schema"),
                )
            })?;
            Ok(ToolDefinitionV1 {
                name: name.to_owned(),
                description: description.to_owned(),
                input_schema_id: SchemaId::new(format!("annals.{name}.input.v1")),
                input_schema: to_raw_value(input_schema)?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    ToolsetRegistrationV1::new(
        toolset_ref(),
        TOOLSET_DEFINITIONS_SCHEMA,
        ToolsetDefinitionsV1 {
            version: PROTOCOL_VERSION_V1,
            tools,
        },
    )
    .map_err(Into::into)
}

fn toolset_ref() -> ToolsetRef {
    ToolsetRef {
        provider: "annals".to_owned(),
        name: TOOLSET_NAME.to_owned(),
        version: TOOLSET_VERSION,
    }
}

fn tool_call_matches_contract(
    admitted_job_id: &JobId,
    call_job_id: &JobId,
    tool_name: &str,
    arguments_schema_id: &SchemaId,
) -> bool {
    admitted_job_id == call_job_id
        && Tool::from_name(tool_name).is_some()
        && arguments_schema_id.as_str() == format!("annals.{tool_name}.input.v1")
}

async fn read_final_response(
    client: &NucleusClient,
    job_id: &JobId,
    deadline: Instant,
    cancellation_requested: &dyn Fn() -> bool,
) -> AppResult<String> {
    let mut cursor = 0;
    let mut final_response = String::new();
    loop {
        let query = nucleus_core::LogsQueryV1 {
            after: cursor,
            follow: false,
            limit: Some(1_000),
        };
        let logs = await_retryable_read(
            client,
            job_id,
            || client.logs(job_id, &query),
            deadline,
            cancellation_requested,
        )
        .await
        .map_err(|error| client_call_error(error, "model_runner_failed"))?;
        let count = logs.records.len();
        for record in &logs.records {
            if !record
                .schema_id
                .as_str()
                .starts_with("codex.app-server.protocol.")
            {
                return Err(AppError::unexpected(
                    "model_runner_protocol",
                    format!(
                        "Nucleus returned a Codex record under unexpected schema {}",
                        record.schema_id
                    ),
                ));
            }
            let value: Value = serde_json::from_str(record.payload.get()).map_err(|error| {
                AppError::unexpected(
                    "model_runner_protocol",
                    format!("Nucleus returned an invalid Codex protocol record: {error}"),
                )
            })?;
            if value.get("method").and_then(Value::as_str) == Some("item/completed")
                && value.pointer("/params/item/type").and_then(Value::as_str)
                    == Some("agentMessage")
                && let Some(text) = value.pointer("/params/item/text").and_then(Value::as_str)
            {
                text.clone_into(&mut final_response);
            }
        }
        cursor = logs.next_sequence;
        if count < 1_000 {
            return Ok(final_response);
        }
    }
}

fn dispatch_tool_call(
    backend: &mut impl Backend,
    reconciliation_recorded: &mut bool,
    name: &str,
    arguments: Value,
) -> Result<ToolSuccess, ToolFailure> {
    let Some(tool) = Tool::from_name(name) else {
        return Err(ToolFailure::new(
            "unknown_tool",
            format!("unknown Annals tool {name:?}"),
        ));
    };
    if tool.mutates_reconciliation_draft() && *reconciliation_recorded {
        return Err(ToolFailure::new(
            "reconciliation_already_submitted",
            "this liaison session has already recorded its reconciliation",
        ));
    }
    let result = backend.call(tool, arguments);
    if result
        .as_ref()
        .is_ok_and(ToolSuccess::reconciliation_recorded)
    {
        *reconciliation_recorded = true;
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
            let mut value = json!({
                "error": {
                    "code": error.code(),
                    "message": error.message()
                }
            });
            if let Some(details) = error.details() {
                value["error"]["details"] = details.clone();
            }
            (
                serde_json::to_string(&value).unwrap_or_else(|_| {
                    r#"{"error":{"code":"tool_failed","message":"tool failed"}}"#.to_owned()
                }),
                false,
            )
        }
    }
}

fn ensure_before_deadline(deadline: Instant) -> AppResult<()> {
    if Instant::now() >= deadline {
        Err(timeout_error())
    } else {
        Ok(())
    }
}

fn interrupted_error() -> AppError {
    AppError::unexpected("model_runner_interrupted", "model liaison was interrupted")
}

fn timeout_error() -> AppError {
    AppError::unexpected(
        "model_runner_timeout",
        "model liaison exceeded its time limit",
    )
}

fn auth_error(message: &str) -> AppError {
    AppError::unexpected("model_auth_unavailable", message)
}

fn client_error(code: &'static str, error: &ClientError) -> AppError {
    runtime_error(code, &error.to_string())
}

fn runtime_error(code: &'static str, message: &str) -> AppError {
    let normalized = message.to_ascii_lowercase();
    let code = if normalized.contains("refresh_token_reused")
        || normalized.contains("refresh token has already been used")
        || normalized.contains("please log out and sign in again")
        || normalized.contains("401 unauthorized")
        || normalized.contains("authentication") && normalized.contains("unavailable")
    {
        "model_auth_unavailable"
    } else {
        code
    };
    AppError::unexpected(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::thread;

    struct UnusedBackend;

    impl Backend for UnusedBackend {
        fn call(&mut self, _tool: Tool, _arguments: Value) -> Result<ToolSuccess, ToolFailure> {
            panic!("a stalled Nucleus fixture must not reach an Annals tool")
        }
    }

    fn stalled_nucleus() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let socket = directory.path().join("nucleus.sock");
        let listener = UnixListener::bind(&socket)?;
        thread::spawn(move || {
            while let Ok((_stream, _)) = listener.accept() {
                thread::sleep(Duration::from_secs(2));
            }
        });
        Ok((directory, socket))
    }

    #[test]
    fn quality_presets_select_the_expected_model_and_effort() {
        for (quality, model, effort) in [
            (ModelQuality::Low, "gpt-5.6-luna", "medium"),
            (ModelQuality::Medium, "gpt-5.6-terra", "medium"),
            (ModelQuality::High, "gpt-5.6-sol", "max"),
        ] {
            let settings = ModelSettings::new(quality, None);
            assert_eq!(settings.model(), model);
            assert_eq!(settings.reasoning_effort(), effort);
        }
        let custom = ModelSettings::new(ModelQuality::Low, Some("custom-model"));
        assert_eq!(custom.model(), "custom-model");
        assert_eq!(custom.reasoning_effort(), "medium");
    }

    #[test]
    fn toolset_is_derived_from_the_nine_authoritative_definitions() -> AppResult<()> {
        let registration = toolset_registration()?;
        assert_eq!(registration.definitions.tools.len(), 9);
        assert_eq!(registration.toolset, toolset_ref());
        assert_eq!(
            registration.definitions_schema_id.as_str(),
            TOOLSET_DEFINITIONS_SCHEMA
        );
        Ok(())
    }

    #[test]
    fn reused_refresh_token_has_a_stable_authentication_error_code() {
        let error = runtime_error(
            "model_runner_failed",
            "HTTP 401: refresh_token_reused; please log out and sign in again",
        );
        assert_eq!(error.code(), "model_auth_unavailable");
    }

    #[test]
    fn managed_tool_calls_must_match_the_registered_job_and_schema() {
        let job = JobId::new("annals-run");
        assert!(super::tool_call_matches_contract(
            &job,
            &job,
            "work_read",
            &SchemaId::new("annals.work_read.input.v1"),
        ));
        assert!(!super::tool_call_matches_contract(
            &job,
            &JobId::new("annals-other"),
            "work_read",
            &SchemaId::new("annals.work_read.input.v1"),
        ));
        assert!(!super::tool_call_matches_contract(
            &job,
            &job,
            "work_read",
            &SchemaId::new("annals.other.input.v1"),
        ));
        assert!(!super::tool_call_matches_contract(
            &job,
            &job,
            "unknown",
            &SchemaId::new("annals.unknown.input.v1"),
        ));
    }

    #[test]
    fn stalled_nucleus_request_honors_the_runner_deadline() -> Result<(), Box<dyn std::error::Error>>
    {
        let (_directory, socket) = stalled_nucleus()?;
        let runner = Runner::new(socket, Duration::from_millis(300));
        let started = Instant::now();
        let Err(error) = runner.run_liaison(
            &ModelSettings::default(),
            "prompt",
            "stalled-timeout",
            &mut UnusedBackend,
            false,
        ) else {
            return Err("a stalled Nucleus unexpectedly completed".into());
        };
        assert_eq!(error.code(), "model_runner_timeout");
        assert!(started.elapsed() < Duration::from_secs(2));
        Ok(())
    }

    #[test]
    fn stalled_nucleus_request_observes_cancellation() -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, socket) = stalled_nucleus()?;
        let runner = Runner::new(socket, Duration::from_secs(10));
        let started = Instant::now();
        let Err(error) = runner.run_liaison_cancellable(
            &ModelSettings::default(),
            "prompt",
            "stalled-interrupt",
            &mut UnusedBackend,
            false,
            &|| started.elapsed() >= Duration::from_millis(300),
        ) else {
            return Err("a stalled Nucleus ignored cancellation".into());
        };
        assert_eq!(error.code(), "model_runner_interrupted");
        assert!(started.elapsed() < Duration::from_secs(2));
        Ok(())
    }

    #[test]
    fn idempotent_read_transport_errors_retry_until_the_deadline()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let client = NucleusClient::new(directory.path().join("absent-nucleus.sock"))?;
        let job_id = JobId::new("annals-retry-read");
        let started = Instant::now();
        let result = runtime()?.block_on(await_retryable_read(
            &client,
            &job_id,
            || client.get_job(&job_id),
            started + Duration::from_millis(300),
            &|| false,
        ));
        if !matches!(result, Err(ClientCallError::TimedOut)) {
            return Err("a retryable read returned before its deadline".into());
        }
        assert!(started.elapsed() >= Duration::from_millis(250));
        assert!(started.elapsed() < Duration::from_secs(2));
        Ok(())
    }

    #[test]
    fn best_effort_stalled_cancel_is_bounded() -> Result<(), Box<dyn std::error::Error>> {
        let (_directory, socket) = stalled_nucleus()?;
        let runner = Runner::new(socket, Duration::from_secs(10));
        let started = Instant::now();
        runner.cancel_liaison("stalled-cancel");
        assert!(started.elapsed() < Duration::from_secs(2));
        Ok(())
    }
}
