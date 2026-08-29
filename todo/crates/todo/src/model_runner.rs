use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nucleus_client::{ClientError, NucleusClient};
use nucleus_core::{
    AbsolutePath, AgentInvocationV1, BuiltinToolsV1, JobId, JobRequestV1, JobState,
    LaunchContextRegistrationV1, LaunchEnvironmentVariableV1, LogSchemaV1, ModelId,
    PROTOCOL_VERSION_V1, ReasoningEffort, Requester, SchemaId, TimeoutSeconds, ToolCallsQueryV1,
    ToolDefinitionV1, ToolResultV1, ToolsetDefinitionsV1, ToolsetRef, ToolsetRegistrationV1,
    WorkspaceAccess,
};
use serde_json::json;
use serde_json::value::{RawValue, to_raw_value};
use tokio::runtime::Builder;
#[cfg(test)]
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::model::ModelQuality;
use crate::tool_server::{
    self, Backend, Stage, StageContract, ToolFailure, ToolSuccess, WorkspacePolicy,
};

const DEFAULT_TIMEOUT: Duration = Duration::from_hours(1);
const MAILBOX_WAIT_SECONDS: u32 = 1;
const TOOLSET_DEFINITIONS_SCHEMA_ID: &str = "nucleus.toolset-definitions.v1";

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
}

/// Stable requester and job identities are supplied by Todo's stage receipt.
/// The legacy wrapper generates them because schema-v1 creation has no receipt.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct RunIdentity {
    requester_id: String,
    job_id: String,
}

impl RunIdentity {
    #[must_use]
    pub(crate) fn new(requester_id: impl Into<String>, job_id: impl Into<String>) -> Self {
        Self {
            requester_id: requester_id.into(),
            job_id: job_id.into(),
        }
    }

    #[must_use]
    pub(crate) fn requester_id(&self) -> &str {
        &self.requester_id
    }

    #[must_use]
    pub(crate) fn job_id(&self) -> &str {
        &self.job_id
    }

    #[cfg(test)]
    fn fresh(stage: Stage) -> Self {
        let suffix = Uuid::now_v7();
        Self::new(
            format!("todo-{}-request-{suffix}", stage.slug()),
            format!("todo-{}-{suffix}", stage.slug()),
        )
    }
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
    pub(crate) fn for_current_user() -> Self {
        Self::default()
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn new(socket: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            socket: Some(socket.into()),
            timeout,
        }
    }

    /// Run one research liaison from the caller's working directory.
    ///
    /// Nucleus owns Codex authentication and execution. Todo retains the prompt, managed tool,
    /// domain transaction, and the rule that a durable creation outranks a later runtime failure.
    #[cfg(test)]
    pub(crate) fn run_liaison(
        &self,
        settings: &ModelSettings,
        prompt: &str,
        working_directory: &Path,
        backend: &mut impl Backend,
    ) -> AppResult<String> {
        self.run_stage(
            Stage::LegacyCreation,
            &RunIdentity::fresh(Stage::LegacyCreation),
            settings,
            prompt,
            working_directory,
            backend,
        )
    }

    /// Run one immutable Todo model-stage contract.
    ///
    /// New v2 callers persist `identity` before admission so an ambiguous
    /// submit can be retried with the same job ID and request bytes.
    pub(crate) fn run_stage(
        &self,
        stage: Stage,
        identity: &RunIdentity,
        settings: &ModelSettings,
        prompt: &str,
        working_directory: &Path,
        backend: &mut impl Backend,
    ) -> AppResult<String> {
        let contract = tool_server::contract(stage);
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                AppError::unexpected(
                    "model_runner_thread",
                    format!("could not initialize the model liaison runtime: {error}"),
                )
            })?;
        let execution = runtime.block_on(async {
            tokio::time::timeout(self.timeout, async {
                let client = match &self.socket {
                    Some(socket) => NucleusClient::new(socket),
                    None => NucleusClient::for_current_user(),
                }
                .map_err(|error| runtime_client_error("model_runner_spawn", &error))?;
                self.run_with_client(
                    &client,
                    &contract,
                    identity,
                    settings,
                    prompt,
                    working_directory,
                    backend,
                )
                .await
            })
            .await
        });
        let result = match execution {
            Ok(result) => result,
            Err(_) => Err(timeout_failure()),
        };
        result.map_err(|error| runtime_error(error.code, &error.message))
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    async fn run_with_client(
        &self,
        client: &NucleusClient,
        contract: &StageContract,
        identity: &RunIdentity,
        settings: &ModelSettings,
        prompt: &str,
        working_directory: &Path,
        backend: &mut impl Backend,
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

        let registration = toolset_registration(contract)?;
        for schema in tool_schemas(contract)? {
            client
                .register_schema(&schema)
                .await
                .map_err(|error| runtime_client_error("model_runner_tool_schema", &error))?;
        }
        client
            .register_toolset(&registration)
            .await
            .map_err(|error| runtime_client_error("model_runner_tool_schema", &error))?;

        let requester = Requester {
            program: "todo".to_owned(),
            id: identity.requester_id().to_owned(),
        };
        let launch_context = if contract.local_execution {
            Some(
                client
                    .register_launch_context(&LaunchContextRegistrationV1 {
                        version: PROTOCOL_VERSION_V1,
                        requester: requester.clone(),
                        environment: if contract.inherit_environment {
                            launch_environment()?
                        } else {
                            Vec::new()
                        },
                    })
                    .await
                    .map_err(|error| runtime_client_error("model_runner_spawn", &error))?,
            )
        } else {
            None
        };
        let mut invocation = AgentInvocationV1::new(
            "codex",
            ModelId::new(settings.model()),
            AbsolutePath::new(working_directory),
            match contract.workspace_policy {
                WorkspacePolicy::None => WorkspaceAccess::None,
                WorkspacePolicy::ReadOnly => WorkspaceAccess::ReadOnly,
            },
            BuiltinToolsV1 {
                local_execution: contract.local_execution,
                web_search: contract.web_search,
            },
            TimeoutSeconds::new(self.timeout.as_secs().max(1)),
        );
        invocation.reasoning_effort = Some(settings.reasoning_effort());
        invocation.toolset = Some(registration.toolset.clone());
        invocation.launch_context = launch_context.map(|context| context.id);
        let mut request = JobRequestV1::new(
            JobId::new(identity.job_id()),
            contract.label,
            requester.clone(),
            contract.instructions,
            prompt,
            invocation,
        );
        request.developer_instructions = Some(contract.developer_instructions.to_owned());

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
        let mut cached_results: BTreeMap<String, CachedToolResult> = BTreeMap::new();

        loop {
            let calls = loop {
                match client
                    .pending_tool_calls(
                        &job_id,
                        &ToolCallsQueryV1 {
                            after: tool_after,
                            wait_seconds: MAILBOX_WAIT_SECONDS,
                        },
                    )
                    .await
                {
                    Ok(calls) => break calls,
                    Err(ClientError::Transport { .. }) => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    Err(error) => {
                        return Err(runtime_client_error("model_runner_protocol", &error));
                    }
                }
            };
            for pending in calls.calls {
                let call = pending.call;
                let Some(definition) = contract.tool_named(&call.tool_name) else {
                    return Err(RuntimeFailure::new(
                        "model_runner_protocol",
                        "Nucleus returned a tool call outside the admitted Todo contract",
                    ));
                };
                if call.job_id != job_id
                    || call.arguments_schema_id.as_str() != definition.input_schema_id
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
                    let result = tool_server::dispatch(
                        backend,
                        contract,
                        call.id.as_str(),
                        &call.tool_name,
                        arguments,
                    );
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
                    result_schema_id: SchemaId::new(contract.result_schema_id),
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

            let job = loop {
                match client.get_job(&job_id).await {
                    Ok(job) => break job,
                    Err(ClientError::Transport { .. }) => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    Err(error) => {
                        return Err(runtime_client_error("model_runner_protocol", &error));
                    }
                }
            };
            if !job.summary.state.is_terminal() {
                continue;
            }
            return match job.summary.state {
                JobState::Completed => job
                    .attempts
                    .last()
                    .and_then(|attempt| attempt.output.as_ref())
                    .map(|output| output.final_message.clone())
                    .ok_or_else(|| {
                        RuntimeFailure::new(
                            "model_runner_protocol",
                            "Nucleus completed the liaison without structured attempt output",
                        )
                    }),
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

fn tool_schemas(contract: &StageContract) -> Result<Vec<LogSchemaV1>, RuntimeFailure> {
    let mut schemas = contract
        .tools
        .iter()
        .map(|tool| {
            let schema = to_raw_value(&tool.input_schema).map_err(|error| {
                RuntimeFailure::new(
                    "model_runner_tool_schema",
                    format!(
                        "could not encode {} input schema: {error}",
                        tool.tool.name()
                    ),
                )
            })?;
            Ok(LogSchemaV1::new(
                tool.input_schema_id,
                format!("Todo {} input", tool.tool.name()),
                "1",
                "application/schema+json",
                "todo",
                schema,
            ))
        })
        .collect::<Result<Vec<_>, RuntimeFailure>>()?;
    let result = to_raw_value(&contract.result_schema).map_err(|error| {
        RuntimeFailure::new(
            "model_runner_tool_schema",
            format!(
                "could not encode {} result schema: {error}",
                contract.toolset_name
            ),
        )
    })?;
    schemas.push(LogSchemaV1::new(
        contract.result_schema_id,
        format!("Todo {} result", contract.toolset_name),
        "1",
        "application/schema+json",
        "todo",
        result,
    ));
    Ok(schemas)
}

fn toolset_registration(contract: &StageContract) -> Result<ToolsetRegistrationV1, RuntimeFailure> {
    let definitions = ToolsetDefinitionsV1 {
        version: PROTOCOL_VERSION_V1,
        tools: contract
            .tools
            .iter()
            .map(|tool| {
                Ok(ToolDefinitionV1 {
                    name: tool.tool.name().to_owned(),
                    description: tool.description.to_owned(),
                    input_schema_id: SchemaId::new(tool.input_schema_id),
                    input_schema: to_raw_value(&tool.input_schema).map_err(|error| {
                        RuntimeFailure::new(
                            "model_runner_tool_schema",
                            format!(
                                "could not encode {} input schema: {error}",
                                tool.tool.name()
                            ),
                        )
                    })?,
                })
            })
            .collect::<Result<Vec<_>, RuntimeFailure>>()?,
    };
    ToolsetRegistrationV1::new(
        ToolsetRef {
            provider: "todo".to_owned(),
            name: contract.toolset_name.to_owned(),
            version: contract.toolset_version,
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

fn runtime_error(code: &'static str, message: &str) -> AppError {
    AppError::unexpected(code, message)
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::thread;
    use std::time::Duration;

    use serde_json::{Value, json};

    use crate::model::ModelQuality;
    use crate::tool_server::{self, Backend, Call, Stage, ToolFailure, ToolSuccess};

    use super::{
        ModelSettings, RunIdentity, Runner, model_tool_result, tool_schemas, toolset_registration,
    };

    const TOOL_INPUT_SCHEMA_ID: &str = "todo.tool.create-todo.input.v1";
    const TOOL_RESULT_SCHEMA_ID: &str = "todo.tool.create-todo.result.v1";

    #[test]
    fn run_identity_exposes_the_persisted_requester_and_job_ids() {
        let identity = RunIdentity::new("todo-routing-request-r7-v2", "todo-routing-r7-v2");
        assert_eq!(identity.requester_id(), "todo-routing-request-r7-v2");
        assert_eq!(identity.job_id(), "todo-routing-r7-v2");
    }

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
        let contract = tool_server::contract(Stage::LegacyCreation);
        let Ok(registration) = toolset_registration(&contract) else {
            panic!("valid Todo toolset was rejected");
        };
        assert_eq!(registration.toolset.provider, "todo");
        assert_eq!(registration.toolset.name, "research-liaison");
        assert_eq!(registration.definitions.tools.len(), 1);
        let tool = &registration.definitions.tools[0];
        assert_eq!(tool.name, "create_todo");
        assert_eq!(tool.input_schema_id.as_str(), TOOL_INPUT_SCHEMA_ID);
        let Ok(schemas) = tool_schemas(&contract) else {
            panic!("valid Todo schemas were rejected");
        };
        assert_eq!(schemas[0].id.as_str(), TOOL_INPUT_SCHEMA_ID);
        assert_eq!(schemas[1].id.as_str(), TOOL_RESULT_SCHEMA_ID);
    }

    #[derive(Default)]
    struct StubBackend {
        calls: Vec<(String, Call)>,
        reject_next: bool,
        created: bool,
    }

    impl Backend for StubBackend {
        fn call(&mut self, tool_call_id: &str, call: Call) -> Result<ToolSuccess, ToolFailure> {
            assert!(matches!(call, Call::CreateTodo(_)));
            self.calls.push((tool_call_id.to_owned(), call));
            if self.reject_next {
                self.reject_next = false;
                Err(ToolFailure::new("invalid_todo", "the title is blank"))
            } else if self.created {
                Err(ToolFailure::new(
                    "todo_already_created",
                    "this session has already created its todo",
                ))
            } else {
                self.created = true;
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
            created: false,
        };
        let contract = tool_server::contract(Stage::LegacyCreation);
        assert!(
            tool_server::dispatch(
                &mut backend,
                &contract,
                "call-rejected",
                "create_todo",
                json!({ "title": "Rejected", "note": "note" }),
            )
            .is_err()
        );
        assert!(!backend.created);
        assert!(
            tool_server::dispatch(
                &mut backend,
                &contract,
                "call-created",
                "create_todo",
                json!({ "title": "Title", "note": "Note" }),
            )
            .is_ok()
        );
        assert!(backend.created);
        let duplicate = tool_server::dispatch(
            &mut backend,
            &contract,
            "call-duplicate",
            "create_todo",
            json!({ "title": "Second", "note": "Second" }),
        );
        assert_eq!(
            duplicate.as_ref().err().map(ToolFailure::code),
            Some("todo_already_created")
        );
        assert_eq!(backend.calls.len(), 3);
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
    fn nucleus_job_uses_durable_state_and_attempt_output() -> Result<(), Box<dyn std::error::Error>>
    {
        exercise_fake_nucleus()
    }

    fn exercise_fake_nucleus() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let socket = directory.path().join("nucleus.sock");
        let listener = UnixListener::bind(&socket)?;
        let working = directory.path().join("research-root");
        std::fs::create_dir(&working)?;
        let working = std::fs::canonicalize(working)?;
        let expected_working = working.display().to_string();
        let server = thread::spawn(move || serve_fake_nucleus(&listener, &expected_working));

        let settings = ModelSettings::new(ModelQuality::Low, Some("custom-model"));
        let runner = Runner::new(&socket, Duration::from_secs(5));
        let mut backend = StubBackend::default();
        let diagnostic =
            runner.run_liaison(&settings, "Research this need", &working, &mut backend)?;
        assert_eq!(diagnostic, "created");
        assert_eq!(backend.calls.len(), 1);
        let (tool_call_id, Call::CreateTodo(arguments)) = &backend.calls[0] else {
            panic!("runner dispatched an unexpected tool");
        };
        assert_eq!(tool_call_id, "call-1");
        assert_eq!(arguments.title, "Actionable title");

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
                assert_eq!(
                    request["developerInstructions"],
                    tool_server::contract(Stage::LegacyCreation).developer_instructions
                );
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
                    "answeredAt": "2026-08-27T00:00:01Z"
                })
            }
            8 => {
                assert!(request_line.starts_with("GET /v1/jobs/"));
                assert!(!request_line.contains("/logs"));
                fake_job_response(submitted.as_ref(), job_id.as_ref(), false)
            }
            9 => {
                assert!(request_line.contains("/tool-calls?"));
                let id = required_test_value(job_id.as_ref());
                json!({
                    "version": 1,
                    "jobId": id,
                    "calls": [],
                    "nextSequence": 1
                })
            }
            10 => {
                assert!(request_line.starts_with("GET /v1/jobs/"));
                assert!(!request_line.contains("/logs"));
                fake_job_response(submitted.as_ref(), job_id.as_ref(), true)
            }
            _ => panic!("unexpected fake Nucleus request"),
        }
    }

    fn fake_job_response(
        submitted: Option<&Value>,
        job_id: Option<&String>,
        completed: bool,
    ) -> Value {
        let id = required_test_value(job_id);
        let request = required_test_value(submitted);
        let mut summary = json!({
            "version": 1,
            "id": id,
            "label": "Todo research liaison",
            "requester": request["requester"],
            "state": if completed { "completed" } else { "running" },
            "requestDigest": "sha256:fake",
            "createdAt": "2026-08-27T00:00:00Z",
            "updatedAt": "2026-08-27T00:00:01Z",
            "currentAttemptId": "attempt-1"
        });
        let mut attempt = json!({
            "version": 1,
            "id": "attempt-1",
            "jobId": id,
            "ordinal": 1,
            "harness": {
                "harness": "codex",
                "harnessVersion": "0.146.0",
                "adapterVersion": "test"
            },
            "state": if completed { "completed" } else { "running" },
            "createdAt": "2026-08-27T00:00:00Z",
            "startedAt": "2026-08-27T00:00:00Z"
        });
        if completed {
            summary["completedAt"] = json!("2026-08-27T00:00:01Z");
            attempt["completedAt"] = json!("2026-08-27T00:00:01Z");
            attempt["terminalReason"] = json!("completed");
            attempt["terminalMessage"] = json!("Codex turn completed");
            attempt["output"] = json!({
                "threadId": "thread-1",
                "turnId": "turn-1",
                "finalMessage": "created"
            });
        }
        json!({
            "version": 1,
            "summary": summary,
            "request": request,
            "attempts": [attempt]
        })
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
