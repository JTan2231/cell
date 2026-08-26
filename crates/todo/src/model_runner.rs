use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::error::{AppError, AppResult};
use crate::model::ModelQuality;
use crate::tool_server::{self, Backend, Tool, ToolFailure, ToolSuccess};

const DEFAULT_TIMEOUT: Duration = Duration::from_hours(1);
const PROTOCOL_POLL_INTERVAL: Duration = Duration::from_millis(250);
const DEFAULT_MAX_STDOUT_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_STDERR_TAIL_BYTES: usize = 64 * 1024;
const MAX_MODEL_CATALOG_BYTES: usize = 4 * 1024 * 1024;

// Account-backed integrations and state-changing Codex features are intentionally absent. Shell
// inspection and standalone web search remain enabled for research.
const DISABLED_FEATURES: &[&str] = &[
    "apps",
    "auth_elicitation",
    "browser_use",
    "browser_use_external",
    "browser_use_full_cdp_access",
    "computer_use",
    "deferred_executor",
    "enable_mcp_apps",
    "goals",
    "image_generation",
    "in_app_browser",
    "multi_agent",
    "multi_agent_v2",
    "plugins",
    "remote_plugin",
    "request_permissions_tool",
    "skill_mcp_dependency_install",
    "skill_search",
    "token_budget",
    "tool_call_mcp_elicitation",
    "tool_suggest",
];

const CONFIG_OVERRIDES: &[&str] = &[
    "agents.enabled=false",
    "cli_auth_credentials_store=\"file\"",
    "features.code_mode_host=true",
    "include_apps_instructions=false",
    "include_collaboration_mode_instructions=false",
    "include_environment_context=true",
    "include_permissions_instructions=true",
    "orchestrator.mcp.enabled=false",
    "orchestrator.skills.enabled=false",
    "skills.bundled.enabled=false",
    "skills.include_instructions=false",
    "tools.experimental_request_user_input.enabled=false",
    "tools.update_plan.enabled=false",
    "web_search=\"live\"",
    "sandbox_permissions=[\"disk-full-read-access\"]",
    "shell_environment_policy.inherit=\"all\"",
];

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct ModelSettings {
    model: String,
    reasoning_effort: &'static str,
}

impl ModelSettings {
    #[must_use]
    pub(crate) fn new(quality: ModelQuality, model: Option<&str>) -> Self {
        let (preset_model, reasoning_effort) = match quality {
            ModelQuality::Low => ("gpt-5.6-luna", "medium"),
            ModelQuality::Medium => ("gpt-5.6-terra", "medium"),
            ModelQuality::High => ("gpt-5.6-sol", "max"),
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
    pub(crate) fn reasoning_effort(&self) -> &str {
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
    program: PathBuf,
    timeout: Duration,
    max_stdout_bytes: usize,
    stderr_tail_bytes: usize,
}

impl Default for Runner {
    fn default() -> Self {
        Self {
            program: PathBuf::from("codex"),
            timeout: DEFAULT_TIMEOUT,
            max_stdout_bytes: DEFAULT_MAX_STDOUT_BYTES,
            stderr_tail_bytes: DEFAULT_STDERR_TAIL_BYTES,
        }
    }
}

impl Runner {
    #[must_use]
    pub(crate) fn for_program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            ..Self::default()
        }
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn new(program: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            program: program.into(),
            timeout,
            ..Self::default()
        }
    }

    /// Run one research liaison from the caller's working directory.
    ///
    /// The model can inspect the local filesystem and use live web search. Its shell remains under
    /// Codex's read-only sandbox, and `create_todo` is its only state-changing tool. The returned
    /// prose is diagnostic; callers must treat a durable backend creation as authoritative.
    pub(crate) fn run_liaison(
        &self,
        settings: &ModelSettings,
        prompt: &str,
        working_directory: &Path,
        backend: &mut impl Backend,
        forward_stderr: bool,
    ) -> AppResult<String> {
        let deadline = Instant::now() + self.timeout;
        let temporary = TemporaryDirectory::create()?;
        let codex_home = temporary.create_subdirectory("codex-home")?;
        copy_codex_auth(&codex_home)?;
        let catalog_path =
            self.write_research_catalog(temporary.path(), &codex_home, settings.model(), deadline)?;
        let dynamic_tools = dynamic_tool_specs()?;

        let mut command = Command::new(&self.program);
        for feature in DISABLED_FEATURES {
            command.args(["--disable", feature]);
        }
        for setting in CONFIG_OVERRIDES {
            command.args(["-c", setting]);
        }
        let catalog_setting = format!(
            "model_catalog_json={}",
            serde_json::to_string(&catalog_path.display().to_string())?
        );
        command
            .args(["-c", &catalog_setting, "app-server", "--stdio"])
            .current_dir(working_directory)
            .env("CODEX_HOME", &codex_home)
            .env_remove("CODEX_EXEC_SERVER_URL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);

        self.run_app_server(
            command,
            working_directory,
            settings,
            prompt,
            &dynamic_tools,
            backend,
            forward_stderr,
            deadline,
        )
    }

    fn write_research_catalog(
        &self,
        directory: &Path,
        codex_home: &Path,
        model: &str,
        deadline: Instant,
    ) -> AppResult<PathBuf> {
        if Instant::now() >= deadline {
            return Err(timeout_error());
        }
        let mut command = Command::new(&self.program);
        command
            .args(["debug", "models", "--bundled"])
            .env("CODEX_HOME", codex_home)
            .env_remove("CODEX_EXEC_SERVER_URL")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command.spawn().map_err(|error| {
            AppError::unexpected(
                "model_runner_catalog",
                format!(
                    "could not read the bundled model catalog from {}: {error}",
                    self.program.display()
                ),
            )
        })?;
        let group = child.id();
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            terminate(&mut child, group);
            let _ = child.wait();
            return Err(AppError::unexpected(
                "model_runner_catalog",
                "the bundled model catalog command did not provide its configured pipes",
            ));
        };

        let output = thread::spawn(move || read_limited_output(stdout, MAX_MODEL_CATALOG_BYTES));
        let stderr_limit = self.stderr_tail_bytes;
        let diagnostics = thread::spawn(move || read_stderr(stderr, stderr_limit, false));
        let status = wait_for_child(&mut child, deadline);
        terminate(&mut child, group);
        let _ = child.wait();

        let output = output
            .join()
            .map_err(|_| {
                AppError::unexpected(
                    "model_runner_thread",
                    "model catalog output worker panicked",
                )
            })?
            .map_err(|error| {
                AppError::unexpected(
                    "model_runner_catalog",
                    format!("could not read the bundled model catalog: {error}"),
                )
            })?;
        let diagnostics = diagnostics
            .join()
            .map_err(|_| {
                AppError::unexpected(
                    "model_runner_thread",
                    "model catalog diagnostics worker panicked",
                )
            })?
            .map_err(|error| {
                AppError::unexpected(
                    "model_runner_catalog",
                    format!("could not read bundled model catalog diagnostics: {error}"),
                )
            })?;
        let status = status?;
        if !status.success() {
            return Err(runtime_error(
                "model_runner_catalog",
                &format!("could not read the bundled model catalog: {status}"),
                &diagnostics,
            ));
        }
        if output.exceeded_limit {
            return Err(AppError::unexpected(
                "model_runner_catalog",
                "the bundled model catalog was unexpectedly large",
            ));
        }

        let catalog = research_model_catalog(&output.bytes, model)?;
        let path = directory.join("models.json");
        fs::write(&path, serde_json::to_vec(&catalog)?)?;
        Ok(path)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_app_server(
        &self,
        mut command: Command,
        working_directory: &Path,
        settings: &ModelSettings,
        prompt: &str,
        dynamic_tools: &[Value],
        backend: &mut impl Backend,
        forward_stderr: bool,
        deadline: Instant,
    ) -> AppResult<String> {
        if Instant::now() >= deadline {
            return Err(timeout_error());
        }
        let program = command.get_program().display().to_string();
        let mut child = command.spawn().map_err(|error| {
            AppError::unexpected(
                "model_runner_spawn",
                format!("could not start model runner {program}: {error}"),
            )
        })?;
        let group = child.id();
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            terminate(&mut child, group);
            return Err(AppError::unexpected(
                "model_runner_pipe",
                "model runner did not provide all configured pipes",
            ));
        };

        let (output_sender, output_receiver) = mpsc::channel();
        let output_limit = self.max_stdout_bytes;
        let output =
            thread::spawn(move || read_protocol_lines(stdout, output_limit, &output_sender));
        let stderr_limit = self.stderr_tail_bytes;
        let diagnostics = thread::spawn(move || read_stderr(stderr, stderr_limit, forward_stderr));

        let result = ProtocolClient {
            stdin,
            output: output_receiver,
            deadline,
            backend,
            todo_created: false,
            final_response: String::new(),
        }
        .run(working_directory, settings, prompt, dynamic_tools);

        terminate(&mut child, group);
        let _ = child.wait();
        let output_result = output.join().map_err(|_| {
            AppError::unexpected("model_runner_thread", "model runner output worker panicked")
        })?;
        let diagnostics = diagnostics.join().map_err(|_| {
            AppError::unexpected(
                "model_runner_thread",
                "model runner diagnostics worker panicked",
            )
        })??;
        if let Err(error) = output_result {
            return Err(runtime_error(
                "model_runner_protocol",
                &format!("could not read model runner protocol output: {error}"),
                &diagnostics,
            ));
        }
        result.map_err(|error| runtime_error(error.code, &error.message, &diagnostics))
    }
}

struct ProtocolClient<'a, B> {
    stdin: ChildStdin,
    output: Receiver<io::Result<String>>,
    deadline: Instant,
    backend: &'a mut B,
    todo_created: bool,
    final_response: String,
}

impl<B: Backend> ProtocolClient<'_, B> {
    fn run(
        mut self,
        working_directory: &Path,
        settings: &ModelSettings,
        prompt: &str,
        dynamic_tools: &[Value],
    ) -> Result<String, RuntimeFailure> {
        self.request(
            0,
            "initialize",
            &json!({
                "clientInfo": {
                    "name": "todo",
                    "title": "Todo",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": { "experimentalApi": true }
            }),
        )?;
        self.notify("initialized", None)?;
        self.ensure_no_mcp_servers()?;
        let thread = self.request(
            2,
            "thread/start",
            &json!({
                "model": settings.model(),
                "cwd": working_directory.display().to_string(),
                "approvalPolicy": "never",
                "sandbox": "read-only",
                "baseInstructions": tool_server::instructions(),
                "developerInstructions": "Research the direction thoroughly. You may read accessible local and web material, but must not modify anything. Record exactly one todo using only the supplied create_todo tool.",
                "ephemeral": true,
                "dynamicTools": dynamic_tools
            }),
        )?;
        let thread_id = required_string(&thread, "/thread/id", "thread/start response")?;
        let turn = self.request(
            3,
            "turn/start",
            &json!({
                "threadId": thread_id,
                "input": [{ "type": "text", "text": prompt, "textElements": [] }],
                "effort": settings.reasoning_effort()
            }),
        )?;
        let turn_id = required_string(&turn, "/turn/id", "turn/start response")?;

        loop {
            let message = self.receive()?;
            if self.handle_server_request(&message)? {
                continue;
            }
            self.record_agent_message(&message);
            if message.get("method").and_then(Value::as_str) != Some("turn/completed") {
                continue;
            }
            let completed_thread = message.pointer("/params/threadId").and_then(Value::as_str);
            let completed_turn = message.pointer("/params/turn/id").and_then(Value::as_str);
            if completed_thread != Some(thread_id.as_str())
                || completed_turn != Some(turn_id.as_str())
            {
                continue;
            }
            match message
                .pointer("/params/turn/status")
                .and_then(Value::as_str)
            {
                Some("completed") => return Ok(self.final_response),
                Some(status) => {
                    let detail = message
                        .pointer("/params/turn/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("no error detail was provided");
                    return Err(RuntimeFailure::new(
                        "model_runner_failed",
                        format!("model liaison ended with status {status}: {detail}"),
                    ));
                }
                None => {
                    return Err(RuntimeFailure::new(
                        "model_runner_protocol",
                        "turn/completed omitted the turn status",
                    ));
                }
            }
        }
    }

    fn ensure_no_mcp_servers(&mut self) -> Result<(), RuntimeFailure> {
        let response = self.request(
            1,
            "mcpServerStatus/list",
            &json!({
                "cursor": null,
                "limit": null,
                "detail": "toolsAndAuthOnly",
                "threadId": null
            }),
        )?;
        let servers = response
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                RuntimeFailure::new(
                    "model_runner_protocol",
                    "mcpServerStatus/list response omitted its data array",
                )
            })?;
        if servers.is_empty() {
            Ok(())
        } else {
            Err(RuntimeFailure::new(
                "model_runner_tool_inventory",
                "the research liaison unexpectedly loaded an MCP server",
            ))
        }
    }

    fn request(&mut self, id: i64, method: &str, params: &Value) -> Result<Value, RuntimeFailure> {
        self.write(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;
        loop {
            let message = self.receive()?;
            if self.handle_server_request(&message)? {
                continue;
            }
            self.record_agent_message(&message);
            if message.get("id") != Some(&json!(id)) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let detail = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown JSON-RPC error");
                return Err(RuntimeFailure::new(
                    "model_runner_failed",
                    format!("{method} failed: {detail}"),
                ));
            }
            return message.get("result").cloned().ok_or_else(|| {
                RuntimeFailure::new(
                    "model_runner_protocol",
                    format!("{method} response omitted result"),
                )
            });
        }
    }

    fn notify(&mut self, method: &str, params: Option<Value>) -> Result<(), RuntimeFailure> {
        let mut message = json!({ "jsonrpc": "2.0", "method": method });
        if let Some(params) = params {
            message["params"] = params;
        }
        self.write(&message)
    }

    fn receive(&self) -> Result<Value, RuntimeFailure> {
        let line = loop {
            let remaining = self
                .deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(timeout_failure)?;
            let wait = remaining.min(PROTOCOL_POLL_INTERVAL);
            match self.output.recv_timeout(wait) {
                Ok(Ok(line)) => break line,
                Ok(Err(error)) => {
                    return Err(RuntimeFailure::new(
                        "model_runner_protocol",
                        format!("could not read model runner output: {error}"),
                    ));
                }
                Err(RecvTimeoutError::Timeout) if wait < remaining => {}
                Err(RecvTimeoutError::Timeout) => return Err(timeout_failure()),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(RuntimeFailure::new(
                        "model_runner_failed",
                        "model runner exited before completing the liaison turn",
                    ));
                }
            }
        };
        serde_json::from_str(&line).map_err(|error| {
            RuntimeFailure::new(
                "model_runner_protocol",
                format!("model runner emitted invalid JSON-RPC: {error}"),
            )
        })
    }

    fn write(&mut self, message: &Value) -> Result<(), RuntimeFailure> {
        serde_json::to_writer(&mut self.stdin, message).map_err(|error| {
            RuntimeFailure::new(
                "model_runner_stdin",
                format!("could not encode a model runner request: {error}"),
            )
        })?;
        self.stdin.write_all(b"\n").map_err(|error| {
            RuntimeFailure::new(
                "model_runner_stdin",
                format!("could not write to the model runner: {error}"),
            )
        })?;
        self.stdin.flush().map_err(|error| {
            RuntimeFailure::new(
                "model_runner_stdin",
                format!("could not flush a model runner request: {error}"),
            )
        })
    }

    fn handle_server_request(&mut self, message: &Value) -> Result<bool, RuntimeFailure> {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Ok(false);
        };
        let Some(id) = message.get("id").cloned() else {
            return Ok(false);
        };
        if method != "item/tool/call" {
            self.write(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("unsupported server request {method:?}")
                }
            }))?;
            return Err(RuntimeFailure::new(
                "model_runner_protocol",
                format!("model runner requested unsupported operation {method:?}"),
            ));
        }

        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let namespace = params.get("namespace");
        let name = params.get("tool").and_then(Value::as_str);
        let arguments = params.get("arguments").cloned();
        let result = match (namespace, name, arguments) {
            (None | Some(Value::Null), Some(name), Some(arguments)) if arguments.is_object() => {
                dispatch_tool_call(self.backend, &mut self.todo_created, name, arguments)
            }
            _ => Err(ToolFailure::new(
                "invalid_tool_call",
                "Todo tool calls require no namespace, the create_todo name, and object arguments",
            )),
        };
        let (text, success) = model_tool_result(result);
        self.write(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "contentItems": [{ "type": "inputText", "text": text }],
                "success": success
            }
        }))?;
        Ok(true)
    }

    fn record_agent_message(&mut self, message: &Value) {
        if message.get("method").and_then(Value::as_str) != Some("item/completed") {
            return;
        }
        let item = message.pointer("/params/item");
        if item
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str)
            == Some("agentMessage")
            && let Some(text) = item
                .and_then(|value| value.get("text"))
                .and_then(Value::as_str)
        {
            text.clone_into(&mut self.final_response);
        }
    }
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

fn dynamic_tool_specs() -> AppResult<Vec<Value>> {
    tool_server::tool_definitions()
        .into_iter()
        .map(|definition| {
            let name = definition
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::unexpected(
                        "model_runner_tool_schema",
                        "a Todo tool definition omitted its name",
                    )
                })?;
            let description = definition
                .get("description")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::unexpected(
                        "model_runner_tool_schema",
                        format!("Todo tool {name:?} omitted its description"),
                    )
                })?;
            let input_schema = definition.get("inputSchema").cloned().ok_or_else(|| {
                AppError::unexpected(
                    "model_runner_tool_schema",
                    format!("Todo tool {name:?} omitted its input schema"),
                )
            })?;
            Ok(json!({
                "type": "function",
                "name": name,
                "description": description,
                "inputSchema": input_schema,
                "deferLoading": false
            }))
        })
        .collect()
}

fn research_model_catalog(bytes: &[u8], selected_model: &str) -> AppResult<Value> {
    let mut catalog: Value = serde_json::from_slice(bytes).map_err(|error| {
        AppError::unexpected(
            "model_runner_catalog",
            format!("Codex returned an invalid bundled model catalog: {error}"),
        )
    })?;
    let models = catalog
        .get_mut("models")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            AppError::unexpected(
                "model_runner_catalog",
                "Codex's bundled model catalog omitted its models array",
            )
        })?;
    let Some(index) = models
        .iter()
        .position(|model| model.get("slug").and_then(Value::as_str) == Some(selected_model))
    else {
        return Err(AppError::unexpected(
            "model_runner_catalog",
            format!("Codex's bundled model catalog does not contain {selected_model}"),
        ));
    };
    let mut model = models.remove(index);
    let object = model.as_object_mut().ok_or_else(|| {
        AppError::unexpected(
            "model_runner_catalog",
            format!("the {selected_model} catalog entry is not an object"),
        )
    })?;
    let shell_enabled = object
        .get("shell_type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "disabled");
    let web_search_enabled = object
        .get("supports_search_tool")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !shell_enabled || !web_search_enabled {
        return Err(AppError::unexpected(
            "model_runner_catalog",
            format!("{selected_model} does not support the required shell and web research tools"),
        ));
    }
    object.insert("multi_agent_version".to_owned(), json!("disabled"));
    object.insert("apply_patch_tool_type".to_owned(), Value::Null);
    object.insert("include_skills_usage_instructions".to_owned(), json!(false));
    object.insert("experimental_supported_tools".to_owned(), json!([]));
    object.insert("input_modalities".to_owned(), json!(["text"]));
    object.insert(
        "base_instructions".to_owned(),
        json!(tool_server::instructions()),
    );
    object.insert("model_messages".to_owned(), Value::Null);
    *models = vec![model];
    Ok(catalog)
}

fn wait_for_child(
    child: &mut std::process::Child,
    deadline: Instant,
) -> AppResult<std::process::ExitStatus> {
    loop {
        if Instant::now() >= deadline {
            return Err(timeout_error());
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                return Err(AppError::unexpected(
                    "model_runner_wait",
                    format!("could not wait for model runner: {error}"),
                ));
            }
        }
        thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(PROTOCOL_POLL_INTERVAL),
        );
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

fn timeout_error() -> AppError {
    AppError::unexpected(
        "model_runner_timeout",
        "model liaison exceeded its time limit",
    )
}

fn required_string(value: &Value, pointer: &str, context: &str) -> Result<String, RuntimeFailure> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            RuntimeFailure::new(
                "model_runner_protocol",
                format!("{context} omitted {pointer}"),
            )
        })
}

fn copy_codex_auth(isolated_home: &Path) -> AppResult<()> {
    let source_home = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".codex"))
        });
    let Some(source) = source_home.map(|home| home.join("auth.json")) else {
        return Ok(());
    };
    if source.is_file() {
        let mut source = fs::File::open(source)?;
        let mut destination = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(isolated_home.join("auth.json"))?;
        io::copy(&mut source, &mut destination)?;
        destination.flush()?;
    }
    Ok(())
}

fn read_protocol_lines(
    reader: impl Read,
    limit: usize,
    sender: &mpsc::Sender<io::Result<String>>,
) -> io::Result<()> {
    let mut reader = BufReader::new(reader);
    let mut total = 0_usize;
    loop {
        let mut line = String::new();
        let count = reader.read_line(&mut line)?;
        if count == 0 {
            return Ok(());
        }
        total = total.saturating_add(count);
        if total > limit {
            let _ = sender.send(Err(io::Error::other(
                "model liaison produced too much protocol output",
            )));
            return Ok(());
        }
        if sender.send(Ok(line)).is_err() {
            return Ok(());
        }
    }
}

struct LimitedOutput {
    bytes: Vec<u8>,
    exceeded_limit: bool,
}

fn read_limited_output(mut reader: impl Read, limit: usize) -> io::Result<LimitedOutput> {
    let mut bytes = Vec::new();
    let mut exceeded_limit = false;
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(LimitedOutput {
                bytes,
                exceeded_limit,
            });
        }
        let retained = count.min(limit.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..retained]);
        exceeded_limit |= retained < count;
    }
}

fn read_stderr(mut reader: impl Read, limit: usize, forward: bool) -> io::Result<Vec<u8>> {
    let mut tail = Vec::new();
    let mut buffer = [0_u8; 4096];
    let mut terminal = forward.then(|| io::stderr().lock());
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        if let Some(output) = &mut terminal {
            for byte in &buffer[..count] {
                if matches!(byte, b'\n' | 0x20..=0x7e | 0x80..=0xff) {
                    output.write_all(&[*byte])?;
                } else {
                    write!(output, "\\x{byte:02x}")?;
                }
            }
        }
        let chunk = &buffer[..count];
        if chunk.len() >= limit {
            tail.clear();
            tail.extend_from_slice(&chunk[chunk.len() - limit..]);
        } else {
            let excess = tail.len().saturating_add(chunk.len()).saturating_sub(limit);
            if excess > 0 {
                tail.drain(..excess);
            }
            tail.extend_from_slice(chunk);
        }
    }
    Ok(tail)
}

fn terminate(child: &mut Child, group: u32) {
    if !signal_group(group) {
        let _ = child.kill();
    }
}

fn signal_group(group: u32) -> bool {
    Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{group}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
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

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn create() -> AppResult<Self> {
        let base = std::env::temp_dir();
        for counter in 0..1000_u16 {
            let path = base.join(format!("todo-liaison-{}-{counter}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(AppError::unexpected(
            "model_runner_workdir",
            "could not allocate a liaison working directory",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn create_subdirectory(&self, name: &str) -> AppResult<PathBuf> {
        let path = self.path.join(name);
        fs::create_dir(&path)?;
        Ok(path)
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::Duration;

    use serde_json::{Value, json};

    use crate::model::ModelQuality;
    use crate::tool_server::{Backend, Tool, ToolFailure, ToolSuccess};

    use super::{
        CONFIG_OVERRIDES, DISABLED_FEATURES, ModelSettings, Runner, dispatch_tool_call,
        dynamic_tool_specs, research_model_catalog,
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
            assert_eq!(settings.reasoning_effort(), effort);
        }
    }

    #[test]
    fn research_configuration_is_full_read_only_and_web_enabled() {
        for setting in [
            "features.code_mode_host=true",
            "include_environment_context=true",
            "include_permissions_instructions=true",
            "web_search=\"live\"",
            "sandbox_permissions=[\"disk-full-read-access\"]",
            "shell_environment_policy.inherit=\"all\"",
        ] {
            assert!(CONFIG_OVERRIDES.contains(&setting));
        }
        for feature in ["code_mode", "code_mode_host", "code_mode_only"] {
            assert!(!DISABLED_FEATURES.contains(&feature));
        }
    }

    #[test]
    fn catalog_keeps_research_tools_and_removes_patch_tool()
    -> Result<(), Box<dyn std::error::Error>> {
        let selected = "selected-model";
        let catalog = research_model_catalog(
            serde_json::to_string(&json!({
                "models": [{
                    "slug": selected,
                    "tool_mode": "code_mode_only",
                    "shell_type": "shell_command",
                    "supports_search_tool": true,
                    "apply_patch_tool_type": "freeform",
                    "base_instructions": "coding"
                }]
            }))?
            .as_bytes(),
            selected,
        )?;
        let model = &catalog["models"][0];
        assert_eq!(model["tool_mode"], "code_mode_only");
        assert_eq!(model["shell_type"], "shell_command");
        assert_eq!(model["supports_search_tool"], true);
        assert!(model["apply_patch_tool_type"].is_null());
        assert_eq!(model["multi_agent_version"], "disabled");
        assert!(
            model["base_instructions"]
                .as_str()
                .is_some_and(|text| text.contains("research-and-drafting agent"))
        );
        Ok(())
    }

    #[test]
    fn dynamic_inventory_contains_only_managed_creation() -> Result<(), Box<dyn std::error::Error>>
    {
        let tools = dynamic_tool_specs()?;
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "create_todo");
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["deferLoading"], false);
        Ok(())
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
                Ok(ToolSuccess::created(json!({ "id": "t1" })))
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
    fn fake_app_server_receives_policy_and_dispatches_creation()
    -> Result<(), Box<dyn std::error::Error>> {
        let settings = ModelSettings::new(ModelQuality::Low, Some("custom-model"));
        let directory = tempfile::tempdir()?;
        let working = directory.path().join("research-root");
        fs::create_dir(&working)?;
        let working = fs::canonicalize(working)?;
        let program = directory.path().join("fake-codex");
        fs::write(
            &program,
            format!(
                r#"#!/bin/sh
if [ "$1" = "debug" ]; then
  [ -z "${{CODEX_EXEC_SERVER_URL:-}}" ] || exit 9
  printf '%s\n' '{{"models":[{{"slug":"{model}","shell_type":"shell_command","supports_search_tool":true}}]}}'
  exit 0
fi
[ -z "${{CODEX_EXEC_SERVER_URL:-}}" ] || exit 9
case "$*" in
  *'web_search="live"'*) ;;
  *) exit 10 ;;
esac
case "$*" in
  *'sandbox_permissions=["disk-full-read-access"]'*) ;;
  *) exit 10 ;;
esac
case "$*" in
  *'app-server --stdio'*) ;;
  *) exit 10 ;;
esac
[ "$PWD" = "{working}" ] || exit 11
IFS= read -r ignored
printf '%s\n' '{{"jsonrpc":"2.0","id":0,"result":{{}}}}'
IFS= read -r ignored
IFS= read -r ignored
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"data":[],"nextCursor":null}}}}'
IFS= read -r thread_request
case "$thread_request" in
  *'"approvalPolicy":"never"'*) ;;
  *) exit 12 ;;
esac
case "$thread_request" in
  *'"sandbox":"read-only"'*) ;;
  *) exit 12 ;;
esac
case "$thread_request" in
  *'"environments"'*) exit 12 ;;
  *) ;;
esac
case "$thread_request" in
  *'"name":"create_todo"'*) ;;
  *) exit 12 ;;
esac
printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"thread":{{"id":"thread"}}}}}}'
IFS= read -r turn_request
case "$turn_request" in
  *'"effort":"{effort}"'*'Research this need'*) ;;
  *) exit 13 ;;
esac
case "$turn_request" in
  *'"environments"'*) exit 13 ;;
  *) ;;
esac
printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"turn":{{"id":"turn"}}}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","id":20,"method":"item/tool/call","params":{{"namespace":null,"tool":"create_todo","arguments":{{"title":"Actionable title","note":"Researched note"}}}}}}'
IFS= read -r tool_result
case "$tool_result" in
  *'"success":true'*) ;;
  *) exit 14 ;;
esac
printf '%s\n' '{{"jsonrpc":"2.0","method":"item/completed","params":{{"item":{{"type":"agentMessage","text":"created"}}}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","method":"turn/completed","params":{{"threadId":"thread","turn":{{"id":"turn","status":"completed"}}}}}}'
"#,
                model = settings.model(),
                effort = settings.reasoning_effort(),
                working = working.display(),
            ),
        )?;
        let mut permissions = fs::metadata(&program)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&program, permissions)?;

        let runner = Runner::new(&program, Duration::from_secs(3));
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
        Ok(())
    }
}
