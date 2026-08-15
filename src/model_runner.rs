use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{AppError, AppResult};
use crate::tool_server::{self, Backend, Tool, ToolFailure};

const DEFAULT_TIMEOUT: Duration = Duration::from_mins(30);
const DEFAULT_MAX_STDOUT_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_STDERR_TAIL_BYTES: usize = 64 * 1024;
const MAX_MODEL_CATALOG_BYTES: usize = 4 * 1024 * 1024;

const DISABLED_FEATURES: &[&str] = &[
    "apps",
    "auth_elicitation",
    "browser_use",
    "browser_use_external",
    "browser_use_full_cdp_access",
    "code_mode",
    "code_mode_host",
    "code_mode_only",
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
    "standalone_web_search",
    "token_budget",
    "tool_call_mcp_elicitation",
    "tool_suggest",
];

const CONFIG_OVERRIDES: &[&str] = &[
    "agents.enabled=false",
    "cli_auth_credentials_store=\"file\"",
    "include_apps_instructions=false",
    "include_collaboration_mode_instructions=false",
    "include_environment_context=false",
    "include_permissions_instructions=false",
    "orchestrator.mcp.enabled=false",
    "orchestrator.skills.enabled=false",
    "skills.bundled.enabled=false",
    "skills.include_instructions=false",
    "tools.experimental_request_user_input.enabled=false",
    "tools.update_plan.enabled=false",
    "web_search=\"disabled\"",
];

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

    /// Run one isolated Codex liaison whose only model-visible tools are Annals' six tools.
    ///
    /// The returned final response is diagnostic only. Application success is determined by the
    /// reconciliation side effect recorded through `submit_reconciliation`.
    pub(crate) fn run_liaison(
        &self,
        settings: &ModelSettings,
        prompt: &str,
        backend: &mut impl Backend,
        forward_stderr: bool,
    ) -> AppResult<String> {
        let temporary = TemporaryDirectory::create()?;
        let work_dir = temporary.create_subdirectory("work")?;
        let codex_home = temporary.create_subdirectory("codex-home")?;
        copy_codex_auth(&codex_home)?;
        let catalog_path =
            self.write_restricted_catalog(temporary.path(), &codex_home, settings.model())?;
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
            .current_dir(&work_dir)
            .env("CODEX_HOME", &codex_home)
            .env("CODEX_EXEC_SERVER_URL", "none")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);

        self.run_app_server(
            command,
            &work_dir,
            settings,
            prompt,
            &dynamic_tools,
            backend,
            forward_stderr,
        )
    }

    fn write_restricted_catalog(
        &self,
        directory: &Path,
        codex_home: &Path,
        model: &str,
    ) -> AppResult<PathBuf> {
        let output = Command::new(&self.program)
            .args(["debug", "models", "--bundled"])
            .env("CODEX_HOME", codex_home)
            .env("CODEX_EXEC_SERVER_URL", "none")
            .stdin(Stdio::null())
            .output()
            .map_err(|error| {
                AppError::unexpected(
                    "model_runner_catalog",
                    format!(
                        "could not read the bundled model catalog from {}: {error}",
                        self.program.display()
                    ),
                )
            })?;
        if !output.status.success() {
            return Err(runtime_error(
                "model_runner_catalog",
                &format!(
                    "could not read the bundled model catalog: {}",
                    output.status
                ),
                &output.stderr,
            ));
        }
        if output.stdout.len() > MAX_MODEL_CATALOG_BYTES {
            return Err(AppError::unexpected(
                "model_runner_catalog",
                "the bundled model catalog was unexpectedly large",
            ));
        }
        let catalog = restricted_model_catalog(&output.stdout, model)?;
        let path = directory.join("models.json");
        fs::write(&path, serde_json::to_vec(&catalog)?)?;
        Ok(path)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_app_server(
        &self,
        mut command: Command,
        work_dir: &Path,
        settings: &ModelSettings,
        prompt: &str,
        dynamic_tools: &[Value],
        backend: &mut impl Backend,
        forward_stderr: bool,
    ) -> AppResult<String> {
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

        let deadline = Instant::now() + self.timeout;
        let result = ProtocolClient {
            stdin,
            output: output_receiver,
            deadline,
            backend,
            submitted: false,
            final_response: String::new(),
        }
        .run(work_dir, settings, prompt, dynamic_tools);

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
    submitted: bool,
    final_response: String,
}

impl<B: Backend> ProtocolClient<'_, B> {
    fn run(
        mut self,
        work_dir: &Path,
        settings: &ModelSettings,
        prompt: &str,
        dynamic_tools: &[Value],
    ) -> Result<String, RuntimeFailure> {
        self.request(
            0,
            "initialize",
            &json!({
                "clientInfo": {
                    "name": "annals",
                    "title": "Annals",
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
                "cwd": work_dir.display().to_string(),
                "approvalPolicy": "never",
                "sandbox": "read-only",
                "baseInstructions": tool_server::instructions(),
                "developerInstructions": "Use only the six supplied Annals tools. Complete the session with exactly one successful submit_reconciliation call.",
                "ephemeral": true,
                "environments": [],
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
                "effort": settings.reasoning_effort(),
                "environments": []
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
                "the isolated liaison unexpectedly loaded an MCP server",
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
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| {
                RuntimeFailure::new(
                    "model_runner_timeout",
                    "model liaison exceeded its time limit",
                )
            })?;
        let line = match self.output.recv_timeout(remaining) {
            Ok(Ok(line)) => line,
            Ok(Err(error)) => {
                return Err(RuntimeFailure::new(
                    "model_runner_protocol",
                    format!("could not read model runner output: {error}"),
                ));
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(RuntimeFailure::new(
                    "model_runner_timeout",
                    "model liaison exceeded its time limit",
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(RuntimeFailure::new(
                    "model_runner_failed",
                    "model runner exited before completing the liaison turn",
                ));
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
                "error": { "code": -32601, "message": format!("unsupported server request {method:?}") }
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
                dispatch_tool_call(self.backend, &mut self.submitted, name, arguments)
            }
            _ => Err(ToolFailure::new(
                "invalid_tool_call",
                "Annals tool calls require no namespace, a known tool name, and object arguments",
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
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str)
            == Some("agentMessage")
            && let Some(text) = item
                .and_then(|item| item.get("text"))
                .and_then(Value::as_str)
        {
            text.clone_into(&mut self.final_response);
        }
    }
}

fn dispatch_tool_call(
    backend: &mut impl Backend,
    submitted: &mut bool,
    name: &str,
    arguments: Value,
) -> Result<Value, ToolFailure> {
    let Some(tool) = Tool::from_name(name) else {
        return Err(ToolFailure::new(
            "unknown_tool",
            format!("unknown Annals tool {name:?}"),
        ));
    };
    if tool == Tool::SubmitReconciliation && *submitted {
        return Err(ToolFailure::new(
            "reconciliation_already_submitted",
            "this liaison session has already recorded its reconciliation",
        ));
    }
    let result = backend.call(tool, arguments);
    if tool == Tool::SubmitReconciliation && result.is_ok() {
        *submitted = true;
    }
    result
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

fn model_tool_result(result: Result<Value, ToolFailure>) -> (String, bool) {
    match result {
        Ok(value) => (
            serde_json::to_string(&value).unwrap_or_else(|_| "null".to_owned()),
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
            let input_schema = definition.get("inputSchema").cloned().ok_or_else(|| {
                AppError::unexpected(
                    "model_runner_tool_schema",
                    format!("Annals tool {name:?} omitted its input schema"),
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

fn restricted_model_catalog(bytes: &[u8], selected_model: &str) -> AppResult<Value> {
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
    object.insert("tool_mode".to_owned(), json!("direct"));
    object.insert("multi_agent_version".to_owned(), json!("disabled"));
    object.insert("shell_type".to_owned(), json!("disabled"));
    object.insert("supports_parallel_tool_calls".to_owned(), json!(false));
    object.insert("apply_patch_tool_type".to_owned(), Value::Null);
    object.insert("supports_search_tool".to_owned(), json!(false));
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
            let path = base.join(format!("annals-liaison-{}-{counter}", std::process::id()));
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

    use crate::tool_server::{Backend, Tool, ToolFailure};

    use super::{
        CONFIG_OVERRIDES, ModelQuality, ModelSettings, Runner, dispatch_tool_call,
        dynamic_tool_specs, model_tool_result, restricted_model_catalog,
    };

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
        assert_eq!(
            ModelSettings::default(),
            ModelSettings::new(ModelQuality::High, None)
        );
    }

    #[test]
    fn explicit_model_overrides_only_the_preset_model() {
        let settings = ModelSettings::new(ModelQuality::Medium, Some("custom-model"));
        assert_eq!(settings.model(), "custom-model");
        assert_eq!(settings.reasoning_effort(), "medium");
    }

    #[test]
    fn runner_accepts_an_explicit_program_for_isolated_tests() {
        let runner = Runner::new("/usr/bin/false", Duration::from_secs(1));
        assert_eq!(runner.program.to_string_lossy(), "/usr/bin/false");
    }

    #[test]
    fn runner_disables_unrelated_prompt_context() {
        for setting in [
            "include_apps_instructions=false",
            "include_collaboration_mode_instructions=false",
            "include_environment_context=false",
            "include_permissions_instructions=false",
            "skills.bundled.enabled=false",
            "skills.include_instructions=false",
        ] {
            assert!(CONFIG_OVERRIDES.contains(&setting));
        }
    }

    #[test]
    fn model_catalog_disables_every_codex_tool_source() -> Result<(), Box<dyn std::error::Error>> {
        let selected_model = "selected-model";
        let catalog = restricted_model_catalog(
            serde_json::to_string(&json!({
                "models": [{
                    "slug": selected_model,
                    "base_instructions": "coding",
                    "model_messages": { "instructions_template": "coding" }
                }, { "slug": "another-model" }]
            }))?
            .as_bytes(),
            selected_model,
        )?;
        assert_eq!(catalog["models"].as_array().map(Vec::len), Some(1));
        let model = &catalog["models"][0];
        assert_eq!(model["tool_mode"], "direct");
        assert_eq!(model["multi_agent_version"], "disabled");
        assert_eq!(model["shell_type"], "disabled");
        assert_eq!(model["supports_parallel_tool_calls"], false);
        assert!(model["apply_patch_tool_type"].is_null());
        assert_eq!(model["supports_search_tool"], false);
        assert_eq!(model["include_skills_usage_instructions"], false);
        assert!(model["model_messages"].is_null());
        assert!(
            model["base_instructions"]
                .as_str()
                .is_some_and(|instructions| instructions.contains("Annals liaison"))
        );
        Ok(())
    }

    #[test]
    fn dynamic_inventory_is_exactly_the_six_annals_tools() -> Result<(), Box<dyn std::error::Error>>
    {
        let tools = dynamic_tool_specs()?;
        let names = tools
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "work_overview",
                "work_read",
                "work_search",
                "corpus_search",
                "corpus_inspect",
                "submit_reconciliation"
            ]
        );
        assert!(tools.iter().all(|tool| tool["type"] == "function"));
        assert!(tools.iter().all(|tool| tool["deferLoading"] == false));
        Ok(())
    }

    #[derive(Default)]
    struct StubBackend {
        calls: Vec<Tool>,
        reject_next_submission: bool,
    }

    impl Backend for StubBackend {
        fn call(&mut self, tool: Tool, _arguments: Value) -> Result<Value, ToolFailure> {
            self.calls.push(tool);
            if tool == Tool::SubmitReconciliation && self.reject_next_submission {
                self.reject_next_submission = false;
                return Err(ToolFailure::new(
                    "invalid_reconciliation",
                    "the reconciliation is incomplete",
                ));
            }
            Ok(json!({ "ok": true }))
        }
    }

    #[test]
    fn submission_retries_until_the_first_success() {
        let mut backend = StubBackend {
            calls: Vec::new(),
            reject_next_submission: true,
        };
        let mut submitted = false;

        let first = dispatch_tool_call(
            &mut backend,
            &mut submitted,
            "submit_reconciliation",
            json!({}),
        );
        assert_eq!(
            first.as_ref().err().map(ToolFailure::code),
            Some("invalid_reconciliation")
        );
        assert!(!submitted);

        assert!(
            dispatch_tool_call(
                &mut backend,
                &mut submitted,
                "submit_reconciliation",
                json!({}),
            )
            .is_ok()
        );
        assert!(submitted);

        let duplicate = dispatch_tool_call(
            &mut backend,
            &mut submitted,
            "submit_reconciliation",
            json!({}),
        );
        assert_eq!(
            duplicate.as_ref().err().map(ToolFailure::code),
            Some("reconciliation_already_submitted")
        );
        assert_eq!(
            backend.calls,
            [Tool::SubmitReconciliation, Tool::SubmitReconciliation]
        );
    }

    #[test]
    fn direct_tool_failures_preserve_structured_details() -> Result<(), Box<dyn std::error::Error>>
    {
        let failure = ToolFailure::new("ambiguous_quote", "the quote occurs twice")
            .with_details(json!({ "matches": 2 }));
        let (text, success) = model_tool_result(Err(failure));
        let body: Value = serde_json::from_str(&text)?;

        assert!(!success);
        assert_eq!(body["error"]["code"], "ambiguous_quote");
        assert_eq!(body["error"]["message"], "the quote occurs twice");
        assert_eq!(body["error"]["details"]["matches"], 2);
        Ok(())
    }

    #[test]
    fn app_server_dynamic_call_is_dispatched_to_the_backend()
    -> Result<(), Box<dyn std::error::Error>> {
        let settings = ModelSettings::new(ModelQuality::Low, Some("custom-model"));
        let directory = tempfile::tempdir()?;
        let program = directory.path().join("fake-codex");
        fs::write(
            &program,
            format!(
                r#"#!/bin/sh
if [ "$1" = "debug" ]; then
  printf '%s\n' '{{"models":[{{"slug":"{model}"}}]}}'
  exit 0
fi
IFS= read -r ignored
printf '%s\n' '{{"jsonrpc":"2.0","id":0,"result":{{}}}}'
IFS= read -r ignored
IFS= read -r ignored
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"data":[],"nextCursor":null}}}}'
IFS= read -r thread_request
case "$thread_request" in
  *'"model":"{model}"'*) ;;
  *) exit 11 ;;
esac
printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"thread":{{"id":"thread"}}}}}}'
IFS= read -r turn_request
case "$turn_request" in
  *'"effort":"{effort}"'*) ;;
  *) exit 12 ;;
esac
printf '%s\n' '{{"jsonrpc":"2.0","id":3,"result":{{"turn":{{"id":"turn"}}}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","id":20,"method":"item/tool/call","params":{{"threadId":"thread","turnId":"turn","callId":"call","namespace":null,"tool":"work_overview","arguments":{{}}}}}}'
IFS= read -r ignored
printf '%s\n' '{{"jsonrpc":"2.0","method":"item/completed","params":{{"item":{{"type":"agentMessage","text":"diagnostic"}}}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","method":"turn/completed","params":{{"threadId":"thread","turn":{{"id":"turn","status":"completed"}}}}}}'
"#,
                model = settings.model(),
                effort = settings.reasoning_effort()
            ),
        )?;
        let mut permissions = fs::metadata(&program)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&program, permissions)?;

        let runner = Runner::new(&program, Duration::from_secs(2));
        let mut backend = StubBackend::default();
        let diagnostic = runner.run_liaison(&settings, "pointer", &mut backend, false)?;
        assert_eq!(diagnostic, "diagnostic");
        assert_eq!(backend.calls, [Tool::WorkOverview]);
        Ok(())
    }

    #[test]
    fn catalog_failure_has_a_stable_error_code() {
        let runner = Runner::new("/usr/bin/false", Duration::from_secs(1));
        let settings = ModelSettings::default();
        let mut backend = StubBackend::default();
        let Err(error) = runner.run_liaison(&settings, "pointer", &mut backend, false) else {
            panic!("the false runner unexpectedly succeeded");
        };
        assert_eq!(error.code(), "model_runner_catalog");
    }
}
