use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::types::{ThreadTokenUsage, TokenUsageBreakdown};

const ACCOUNT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_ACCOUNT_PROTOCOL_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RateLimitWindow {
    pub(crate) used_percent: i32,
    pub(crate) window_duration_mins: Option<i64>,
    pub(crate) resets_at: Option<i64>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreditsSnapshot {
    pub(crate) has_credits: bool,
    pub(crate) unlimited: bool,
    pub(crate) balance: Option<String>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpendControlLimitSnapshot {
    pub(crate) limit: String,
    pub(crate) used: String,
    pub(crate) remaining_percent: i32,
    pub(crate) resets_at: i64,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RateLimitSnapshot {
    pub(crate) limit_id: Option<String>,
    pub(crate) limit_name: Option<String>,
    pub(crate) primary: Option<RateLimitWindow>,
    pub(crate) secondary: Option<RateLimitWindow>,
    pub(crate) credits: Option<CreditsSnapshot>,
    pub(crate) individual_limit: Option<SpendControlLimitSnapshot>,
    pub(crate) spend_control_reached: Option<bool>,
    pub(crate) plan_type: Option<String>,
    pub(crate) rate_limit_reached_type: Option<String>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RateLimitResetCreditsSummary {
    pub(crate) available_count: i64,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)]
pub(crate) struct AccountRateLimits {
    pub(crate) rate_limits: RateLimitSnapshot,
    pub(crate) rate_limits_by_limit_id: Option<BTreeMap<String, RateLimitSnapshot>>,
    pub(crate) rate_limit_reset_credits: Option<RateLimitResetCreditsSummary>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountTokenUsageSummary {
    pub(crate) lifetime_tokens: Option<i64>,
    pub(crate) peak_daily_tokens: Option<i64>,
    pub(crate) longest_running_turn_sec: Option<i64>,
    pub(crate) current_streak_days: Option<i64>,
    pub(crate) longest_streak_days: Option<i64>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DailyTokenUsage {
    pub(crate) start_date: String,
    pub(crate) tokens: i64,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountTokenUsage {
    pub(crate) summary: AccountTokenUsageSummary,
    pub(crate) daily_usage_buckets: Option<Vec<DailyTokenUsage>>,
    #[serde(flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountSnapshot {
    pub(crate) rate_limits: AccountRateLimits,
    pub(crate) token_activity: Option<AccountTokenUsage>,
    pub(crate) token_activity_error: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) enum TelemetryGap {
    RawResponseOptInNotApplied,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProtocolEvent {
    ThreadStarted {
        thread_id: String,
        model: Option<String>,
    },
    TurnStarted {
        thread_id: String,
        turn_id: String,
        effort: Option<String>,
    },
    TokenUsageUpdated {
        thread_id: String,
        turn_id: String,
        usage: ThreadTokenUsage,
    },
    RawResponseCompleted {
        thread_id: String,
        turn_id: String,
        response_id: String,
        usage: Option<TokenUsageBreakdown>,
    },
    RateLimitsUpdated {
        rate_limits: Box<RateLimitSnapshot>,
    },
    TurnCompleted {
        thread_id: String,
        turn_id: String,
        status: String,
    },
    TelemetryGap(TelemetryGap),
}

/// Run a Codex command without observing its protocol.
///
/// This is used for commands such as `debug models --bundled`. All three standard streams and
/// the inherited environment are left unchanged.
pub(crate) fn run_passthrough(
    codex: &Path,
    arguments: &[OsString],
) -> Result<ExitStatus, ProtocolError> {
    Command::new(codex)
        .args(arguments)
        .status()
        .map_err(|source| ProtocolError::Spawn { source })
}

/// Proxy one Codex app-server stdio session while observing metering events.
///
/// Every server line is observed before it is forwarded. In particular, `turn/completed` is
/// offered to the observer before Annals can receive it and terminate the proxy's process group.
/// If the observer returns an error, observation is disabled for the rest of the process but the
/// protocol continues to be forwarded.
pub(crate) fn run_stdio_proxy<F>(
    codex: &Path,
    arguments: &[OsString],
    mut observer: F,
) -> Result<ExitStatus, ProtocolError>
where
    F: FnMut(&ProtocolEvent) -> io::Result<()>,
{
    let mut child = Command::new(codex)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| ProtocolError::Spawn { source })?;

    let Some(child_stdin) = child.stdin.take() else {
        stop_child(&mut child);
        return Err(ProtocolError::MissingPipe("stdin"));
    };
    let Some(child_stdout) = child.stdout.take() else {
        stop_child(&mut child);
        return Err(ProtocolError::MissingPipe("stdout"));
    };
    let Some(child_stderr) = child.stderr.take() else {
        stop_child(&mut child);
        return Err(ProtocolError::MissingPipe("stderr"));
    };

    let state = Arc::new(Mutex::new(ProtocolState::default()));
    let input_state = Arc::clone(&state);
    let _input = thread::spawn(move || {
        let input = io::stdin();
        forward_client_lines(input.lock(), child_stdin, &input_state)
    });
    let diagnostics = thread::spawn(move || {
        let stderr = io::stderr();
        let mut output = stderr.lock();
        io::copy(&mut BufReader::new(child_stderr), &mut output)?;
        output.flush()
    });

    let forwarding = forward_server_lines(child_stdout, io::stdout().lock(), &state, &mut observer);
    if forwarding.is_err() {
        let _ = child.kill();
    }
    let status = child
        .wait()
        .map_err(|source| ProtocolError::Wait { source })?;
    let diagnostics = diagnostics
        .join()
        .map_err(|_| ProtocolError::WorkerPanicked("stderr"))?;
    diagnostics.map_err(|source| ProtocolError::Forward { source })?;
    forwarding.map_err(|source| ProtocolError::Forward { source })?;
    Ok(status)
}

/// Read account-global `ChatGPT` allowance and token activity without starting a model turn.
pub(crate) fn read_account_snapshot(
    codex: &Path,
    codex_home: Option<&Path>,
) -> Result<AccountSnapshot, ProtocolError> {
    let mut command = Command::new(codex);
    command
        .args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(codex_home) = codex_home {
        command.env("CODEX_HOME", codex_home);
    }
    let mut child = command
        .spawn()
        .map_err(|source| ProtocolError::Spawn { source })?;
    let result = read_account_snapshot_from_child(&mut child);
    stop_child(&mut child);
    result
}

fn read_account_snapshot_from_child(child: &mut Child) -> Result<AccountSnapshot, ProtocolError> {
    let mut stdin = child
        .stdin
        .take()
        .ok_or(ProtocolError::MissingPipe("stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or(ProtocolError::MissingPipe("stdout"))?;
    let (sender, receiver) = mpsc::channel();
    let _reader = thread::spawn(move || {
        read_bounded_protocol_lines(stdout, MAX_ACCOUNT_PROTOCOL_BYTES, &sender)
    });

    write_json_line(
        &mut stdin,
        &json!({
            "method": "initialize",
            "id": 0,
            "params": {
                "clientInfo": {
                    "name": "annals_usage",
                    "title": "Annals Usage",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        }),
    )?;
    let _ = wait_for_result(&receiver, &json!(0), "initialize")?;
    write_json_line(
        &mut stdin,
        &json!({ "method": "initialized", "params": {} }),
    )?;
    write_json_line(
        &mut stdin,
        &json!({ "method": "account/rateLimits/read", "id": 1 }),
    )?;
    let result = wait_for_result(&receiver, &json!(1), "account/rateLimits/read")?;
    let rate_limits =
        serde_json::from_value(result).map_err(|source| ProtocolError::InvalidResult {
            method: "account/rateLimits/read",
            source,
        })?;
    write_json_line(
        &mut stdin,
        &json!({ "method": "account/usage/read", "id": 2 }),
    )?;
    let (token_activity, token_activity_error) =
        match wait_for_result(&receiver, &json!(2), "account/usage/read") {
            Ok(result) => match serde_json::from_value(result) {
                Ok(activity) => (Some(activity), None),
                Err(source) => {
                    let error = ProtocolError::InvalidResult {
                        method: "account/usage/read",
                        source,
                    };
                    (None, Some(error.to_string()))
                }
            },
            Err(error) => (None, Some(error.to_string())),
        };
    Ok(AccountSnapshot {
        rate_limits,
        token_activity,
        token_activity_error,
    })
}

fn write_json_line(writer: &mut impl Write, value: &Value) -> Result<(), ProtocolError> {
    serde_json::to_writer(&mut *writer, value)
        .map_err(|source| ProtocolError::Encode { source })?;
    writer
        .write_all(b"\n")
        .and_then(|()| writer.flush())
        .map_err(|source| ProtocolError::Write { source })
}

fn wait_for_result(
    receiver: &mpsc::Receiver<io::Result<Vec<u8>>>,
    expected_id: &Value,
    method: &'static str,
) -> Result<Value, ProtocolError> {
    loop {
        let line = match receiver.recv_timeout(ACCOUNT_REQUEST_TIMEOUT) {
            Ok(Ok(line)) => line,
            Ok(Err(source)) => return Err(ProtocolError::Read { source }),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(ProtocolError::Timeout { method });
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(ProtocolError::Closed { method });
            }
        };
        let message: Value = serde_json::from_slice(&line)
            .map_err(|source| ProtocolError::InvalidJson { source })?;
        if message.get("id") != Some(expected_id) {
            continue;
        }
        if message.get("error").is_some() {
            return Err(ProtocolError::Rejected { method });
        }
        return message
            .get("result")
            .cloned()
            .ok_or(ProtocolError::MissingResult { method });
    }
}

fn read_bounded_protocol_lines(
    reader: impl Read,
    limit: usize,
    sender: &mpsc::Sender<io::Result<Vec<u8>>>,
) -> io::Result<()> {
    let mut reader = BufReader::new(reader);
    let mut total = 0_usize;
    loop {
        let mut line = Vec::new();
        let count = reader.read_until(b'\n', &mut line)?;
        if count == 0 {
            return Ok(());
        }
        total = total.saturating_add(count);
        if total > limit {
            let _ = sender.send(Err(io::Error::other(
                "Codex account protocol output exceeded its limit",
            )));
            return Ok(());
        }
        if sender.send(Ok(line)).is_err() {
            return Ok(());
        }
    }
}

fn forward_client_lines(
    reader: impl Read,
    mut writer: impl Write,
    state: &Arc<Mutex<ProtocolState>>,
) -> io::Result<()> {
    let mut reader = BufReader::new(reader);
    loop {
        let mut line = Vec::new();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }
        let forwarded = inspect_client_line(&line, state);
        writer.write_all(&forwarded)?;
        writer.flush()?;
    }
}

fn forward_server_lines<F>(
    reader: impl Read,
    mut writer: impl Write,
    state: &Arc<Mutex<ProtocolState>>,
    observer: &mut F,
) -> io::Result<()>
where
    F: FnMut(&ProtocolEvent) -> io::Result<()>,
{
    let mut reader = BufReader::new(reader);
    let mut observing = true;
    loop {
        let mut line = Vec::new();
        if reader.read_until(b'\n', &mut line)? == 0 {
            observe_pending_events(state, observer, &mut observing);
            return Ok(());
        }
        let event = serde_json::from_slice(&line)
            .ok()
            .and_then(|message| lock_state(state).observe_server_message(&message));
        observe_pending_events(state, observer, &mut observing);
        if let Some(event) = event {
            observe(observer, &event, &mut observing);
        }
        writer.write_all(&line)?;
        writer.flush()?;
    }
}

fn inspect_client_line(line: &[u8], state: &Arc<Mutex<ProtocolState>>) -> Vec<u8> {
    let Ok(mut message) = serde_json::from_slice::<Value>(line) else {
        return line.to_vec();
    };
    let is_thread_start = message.get("method").and_then(Value::as_str) == Some("thread/start");
    let mut changed = false;
    if is_thread_start {
        match message.get_mut("params").and_then(Value::as_object_mut) {
            Some(params) => {
                if params.get("experimentalRawEvents") != Some(&Value::Bool(true)) {
                    params.insert("experimentalRawEvents".to_owned(), Value::Bool(true));
                    changed = true;
                }
            }
            None => lock_state(state)
                .pending_events
                .push_back(ProtocolEvent::TelemetryGap(
                    TelemetryGap::RawResponseOptInNotApplied,
                )),
        }
    }
    lock_state(state).observe_client_message(&message);
    if !changed {
        return line.to_vec();
    }

    let Ok(mut encoded) = serde_json::to_vec(&message) else {
        lock_state(state)
            .pending_events
            .push_back(ProtocolEvent::TelemetryGap(
                TelemetryGap::RawResponseOptInNotApplied,
            ));
        return line.to_vec();
    };
    if line.ends_with(b"\r\n") {
        encoded.extend_from_slice(b"\r\n");
    } else if line.ends_with(b"\n") {
        encoded.push(b'\n');
    }
    encoded
}

fn observe_pending_events<F>(
    state: &Arc<Mutex<ProtocolState>>,
    observer: &mut F,
    observing: &mut bool,
) where
    F: FnMut(&ProtocolEvent) -> io::Result<()>,
{
    let events = lock_state(state)
        .pending_events
        .drain(..)
        .collect::<Vec<_>>();
    for event in events {
        observe(observer, &event, observing);
    }
}

fn observe<F>(observer: &mut F, event: &ProtocolEvent, observing: &mut bool)
where
    F: FnMut(&ProtocolEvent) -> io::Result<()>,
{
    if *observing && observer(event).is_err() {
        *observing = false;
    }
}

fn lock_state(state: &Arc<Mutex<ProtocolState>>) -> std::sync::MutexGuard<'_, ProtocolState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug)]
enum PendingRequest {
    ThreadStart {
        id: Value,
        model: Option<String>,
    },
    TurnStart {
        id: Value,
        thread_id: String,
        effort: Option<String>,
    },
}

impl PendingRequest {
    fn id(&self) -> &Value {
        match self {
            Self::ThreadStart { id, .. } | Self::TurnStart { id, .. } => id,
        }
    }
}

#[derive(Debug, Default)]
struct ProtocolState {
    pending_requests: Vec<PendingRequest>,
    pending_events: VecDeque<ProtocolEvent>,
}

impl ProtocolState {
    fn observe_client_message(&mut self, message: &Value) {
        let Some(id) = message.get("id").cloned() else {
            return;
        };
        let pending = match message.get("method").and_then(Value::as_str) {
            Some("thread/start") => Some(PendingRequest::ThreadStart {
                id,
                model: string_at(message, "/params/model"),
            }),
            Some("turn/start") => {
                string_at(message, "/params/threadId").map(|thread_id| PendingRequest::TurnStart {
                    id,
                    thread_id,
                    effort: string_at(message, "/params/effort"),
                })
            }
            _ => None,
        };
        if let Some(pending) = pending {
            self.pending_requests
                .retain(|request| request.id() != pending.id());
            self.pending_requests.push(pending);
        }
    }

    fn observe_server_message(&mut self, message: &Value) -> Option<ProtocolEvent> {
        if let Some(id) = message.get("id")
            && let Some(index) = self
                .pending_requests
                .iter()
                .position(|request| request.id() == id)
        {
            let pending = self.pending_requests.remove(index);
            if message.get("error").is_some() {
                return None;
            }
            return match pending {
                PendingRequest::ThreadStart { model, .. } => {
                    string_at(message, "/result/thread/id")
                        .map(|thread_id| ProtocolEvent::ThreadStarted { thread_id, model })
                }
                PendingRequest::TurnStart {
                    thread_id, effort, ..
                } => string_at(message, "/result/turn/id").map(|turn_id| {
                    ProtocolEvent::TurnStarted {
                        thread_id,
                        turn_id,
                        effort,
                    }
                }),
            };
        }

        match message.get("method").and_then(Value::as_str) {
            Some("thread/tokenUsage/updated") => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Params {
                    thread_id: String,
                    turn_id: String,
                    token_usage: ThreadTokenUsage,
                }
                parse_params::<Params>(message).map(|params| ProtocolEvent::TokenUsageUpdated {
                    thread_id: params.thread_id,
                    turn_id: params.turn_id,
                    usage: params.token_usage,
                })
            }
            Some("rawResponse/completed") => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Params {
                    thread_id: String,
                    turn_id: String,
                    response_id: String,
                    usage: Option<TokenUsageBreakdown>,
                }
                parse_params::<Params>(message).map(|params| ProtocolEvent::RawResponseCompleted {
                    thread_id: params.thread_id,
                    turn_id: params.turn_id,
                    response_id: params.response_id,
                    usage: params.usage,
                })
            }
            Some("account/rateLimits/updated") => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Params {
                    rate_limits: RateLimitSnapshot,
                }
                parse_params::<Params>(message).map(|params| ProtocolEvent::RateLimitsUpdated {
                    rate_limits: Box::new(params.rate_limits),
                })
            }
            Some("turn/completed") => {
                let thread_id = string_at(message, "/params/threadId")?;
                let turn_id = string_at(message, "/params/turn/id")?;
                let status = string_at(message, "/params/turn/status")?;
                Some(ProtocolEvent::TurnCompleted {
                    thread_id,
                    turn_id,
                    status,
                })
            }
            _ => None,
        }
    }
}

fn parse_params<T: for<'de> Deserialize<'de>>(message: &Value) -> Option<T> {
    serde_json::from_value(message.get("params")?.clone()).ok()
}

fn string_at(value: &Value, pointer: &str) -> Option<String> {
    value.pointer(pointer)?.as_str().map(str::to_owned)
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[derive(Debug, Error)]
pub(crate) enum ProtocolError {
    #[error("could not start Codex: {source}")]
    Spawn {
        #[source]
        source: io::Error,
    },
    #[error("Codex did not provide its configured {0} pipe")]
    MissingPipe(&'static str),
    #[error("could not forward the Codex protocol: {source}")]
    Forward {
        #[source]
        source: io::Error,
    },
    #[error("could not wait for Codex: {source}")]
    Wait {
        #[source]
        source: io::Error,
    },
    #[error("the {0} protocol worker panicked")]
    WorkerPanicked(&'static str),
    #[error("could not encode a Codex request: {source}")]
    Encode {
        #[source]
        source: serde_json::Error,
    },
    #[error("could not write a Codex request: {source}")]
    Write {
        #[source]
        source: io::Error,
    },
    #[error("could not read the Codex account protocol: {source}")]
    Read {
        #[source]
        source: io::Error,
    },
    #[error("Codex emitted invalid account protocol JSON: {source}")]
    InvalidJson {
        #[source]
        source: serde_json::Error,
    },
    #[error("Codex did not answer {method} before the timeout")]
    Timeout { method: &'static str },
    #[error("Codex exited before answering {method}")]
    Closed { method: &'static str },
    #[error("Codex rejected {method}")]
    Rejected { method: &'static str },
    #[error("the Codex response to {method} omitted its result")]
    MissingResult { method: &'static str },
    #[error("the Codex response to {method} had an invalid result: {source}")]
    InvalidResult {
        method: &'static str,
        #[source]
        source: serde_json::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    #[test]
    fn thread_start_enables_raw_response_events() -> Result<(), Box<dyn std::error::Error>> {
        let state = Arc::new(Mutex::new(ProtocolState::default()));
        let input = br#"{ "method": "thread/start", "id": 2, "params": { "model": "gpt-test", "ephemeral": true } }
"#;
        let forwarded = inspect_client_line(input, &state);
        let value: Value = serde_json::from_slice(&forwarded)?;
        assert_eq!(value["params"]["experimentalRawEvents"], true);
        assert_eq!(value["params"]["ephemeral"], true);

        let response = json!({ "id": 2, "result": { "thread": { "id": "thread-1" } } });
        assert_eq!(
            lock_state(&state).observe_server_message(&response),
            Some(ProtocolEvent::ThreadStarted {
                thread_id: "thread-1".to_owned(),
                model: Some("gpt-test".to_owned()),
            })
        );
        Ok(())
    }

    #[test]
    fn already_enabled_and_unrelated_lines_are_byte_exact() {
        let state = Arc::new(Mutex::new(ProtocolState::default()));
        for line in [
            b"not JSON\n".as_slice(),
            br#"{ "method": "initialized", "params": {} }
"#,
            br#"{"method":"thread/start","id":2,"params":{"experimentalRawEvents":true}}
"#,
        ] {
            assert_eq!(inspect_client_line(line, &state), line);
        }
    }

    #[test]
    fn malformed_thread_start_reports_a_gap_without_disclosing_payload() {
        let state = Arc::new(Mutex::new(ProtocolState::default()));
        let input = br#"{"method":"thread/start","id":2,"params":null}
"#;
        assert_eq!(inspect_client_line(input, &state), input);
        assert_eq!(
            lock_state(&state).pending_events.pop_front(),
            Some(ProtocolEvent::TelemetryGap(
                TelemetryGap::RawResponseOptInNotApplied
            ))
        );
    }

    #[test]
    fn parses_exact_and_cumulative_usage_categories() {
        let mut state = ProtocolState::default();
        let token_event = json!({
            "method": "thread/tokenUsage/updated",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "tokenUsage": {
                    "last": {
                        "inputTokens": 100,
                        "cachedInputTokens": 60,
                        "cacheWriteInputTokens": 10,
                        "outputTokens": 20,
                        "reasoningOutputTokens": 15,
                        "totalTokens": 120
                    },
                    "total": {
                        "inputTokens": 300,
                        "cachedInputTokens": 160,
                        "cacheWriteInputTokens": 20,
                        "outputTokens": 40,
                        "reasoningOutputTokens": 25,
                        "totalTokens": 340
                    },
                    "modelContextWindow": 200_000
                }
            }
        });
        let Some(ProtocolEvent::TokenUsageUpdated { usage, .. }) =
            state.observe_server_message(&token_event)
        else {
            panic!("expected token usage event");
        };
        assert_eq!(usage.last.cached_input_tokens, 60);
        assert_eq!(usage.last.cache_write_input_tokens, 10);
        assert_eq!(usage.last.reasoning_output_tokens, 15);
        assert_eq!(usage.total.total_tokens, 340);

        let raw = json!({
            "method": "rawResponse/completed",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "responseId": "resp-1",
                "usage": {
                    "inputTokens": 100,
                    "cachedInputTokens": 60,
                    "outputTokens": 20,
                    "reasoningOutputTokens": 15,
                    "totalTokens": 120
                }
            }
        });
        let Some(ProtocolEvent::RawResponseCompleted { usage, .. }) =
            state.observe_server_message(&raw)
        else {
            panic!("expected raw response event");
        };
        assert_eq!(usage.map(|usage| usage.cache_write_input_tokens), Some(0));
    }

    #[test]
    fn correlates_turn_response_and_terminal_notification() {
        let mut state = ProtocolState::default();
        state.observe_client_message(&json!({
            "method": "turn/start",
            "id": "turn-request",
            "params": { "threadId": "thread-1", "effort": "max", "input": ["private"] }
        }));
        assert_eq!(
            state.observe_server_message(
                &json!({ "id": "turn-request", "result": { "turn": { "id": "turn-1" } } })
            ),
            Some(ProtocolEvent::TurnStarted {
                thread_id: "thread-1".to_owned(),
                turn_id: "turn-1".to_owned(),
                effort: Some("max".to_owned()),
            })
        );
        assert_eq!(
            state.observe_server_message(&json!({
                "method": "turn/completed",
                "params": {
                    "threadId": "thread-1",
                    "turn": { "id": "turn-1", "status": "completed", "items": [] }
                },
                "emittedAtMs": 123
            })),
            Some(ProtocolEvent::TurnCompleted {
                thread_id: "thread-1".to_owned(),
                turn_id: "turn-1".to_owned(),
                status: "completed".to_owned(),
            })
        );
    }

    #[test]
    fn account_reader_uses_the_explicit_codex_home() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let codex_home = directory.path().join("state-local-codex-home");
        fs::create_dir(&codex_home)?;
        let fake_codex = directory.path().join("fake-codex");
        fs::write(
            &fake_codex,
            format!(
                r#"#!/bin/sh
set -eu
[ "$1" = "app-server" ]
[ "$2" = "--stdio" ]
[ "${{CODEX_HOME:-}}" = "{}" ]
IFS= read -r initialize
printf '%s\n' '{{"id":0,"result":{{"userAgent":"test","codexHome":"/tmp","platformFamily":"unix","platformOs":"macos"}}}}'
IFS= read -r initialized
IFS= read -r rate_limits
printf '%s\n' '{{"method":"remoteControl/status/changed","params":{{"status":"disabled"}},"emittedAtMs":123}}'
printf '%s\n' '{{"id":1,"result":{{"rateLimits":{{"limitId":"codex","primary":{{"usedPercent":29,"windowDurationMins":10080,"resetsAt":1787803051}},"credits":{{"hasCredits":false,"unlimited":false,"balance":"0"}},"planType":"pro"}},"rateLimitsByLimitId":{{"codex":{{"limitId":"codex","primary":{{"usedPercent":29,"windowDurationMins":10080,"resetsAt":1787803051}},"planType":"pro"}}}},"rateLimitResetCredits":{{"availableCount":1,"credits":[{{"id":"reset-1","title":"Reset"}}]}}}}}}'
IFS= read -r token_activity
printf '%s\n' '{{"id":2,"result":{{"summary":{{"lifetimeTokens":123456,"peakDailyTokens":4567,"longestRunningTurnSec":89,"currentStreakDays":2,"longestStreakDays":4}},"dailyUsageBuckets":[{{"startDate":"2026-08-23","tokens":321}}]}}}}'
"#,
                codex_home.display()
            ),
        )?;
        let mut permissions = fs::metadata(&fake_codex)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&fake_codex, permissions)?;

        let result = read_account_snapshot(&fake_codex, Some(&codex_home))?;
        assert_eq!(
            result.rate_limits.rate_limits.limit_id.as_deref(),
            Some("codex")
        );
        assert_eq!(
            result
                .rate_limits
                .rate_limits_by_limit_id
                .as_ref()
                .and_then(|limits| limits.get("codex"))
                .and_then(|limit| limit.primary.as_ref())
                .map(|window| window.used_percent),
            Some(29)
        );
        assert_eq!(
            result
                .rate_limits
                .rate_limit_reset_credits
                .as_ref()
                .map(|credits| credits.available_count),
            Some(1)
        );
        assert_eq!(
            result
                .rate_limits
                .rate_limit_reset_credits
                .as_ref()
                .and_then(|credits| credits.extra.get("credits"))
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            result
                .token_activity
                .as_ref()
                .and_then(|activity| activity.summary.lifetime_tokens),
            Some(123_456)
        );
        assert_eq!(
            result
                .token_activity
                .as_ref()
                .and_then(|activity| activity.daily_usage_buckets.as_ref())
                .and_then(|buckets| buckets.last())
                .map(|bucket| bucket.tokens),
            Some(321)
        );
        Ok(())
    }

    #[test]
    fn unavailable_token_activity_does_not_hide_allowance() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let fake_codex = directory.path().join("fake-codex");
        fs::write(
            &fake_codex,
            r#"#!/bin/sh
set -eu
IFS= read -r initialize
printf '%s\n' '{"id":0,"result":{"userAgent":"test","codexHome":"/tmp","platformFamily":"unix","platformOs":"macos"}}'
IFS= read -r initialized
IFS= read -r rate_limits
printf '%s\n' '{"id":1,"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":30}}}}'
IFS= read -r token_activity
printf '%s\n' '{"id":2,"error":{"code":-32601,"message":"not available"}}'
"#,
        )?;
        let mut permissions = fs::metadata(&fake_codex)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&fake_codex, permissions)?;

        let result = read_account_snapshot(&fake_codex, None)?;
        assert_eq!(
            result
                .rate_limits
                .rate_limits
                .primary
                .map(|window| window.used_percent),
            Some(30)
        );
        assert!(result.token_activity.is_none());
        assert!(result.token_activity_error.is_some());
        Ok(())
    }
}
