use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{Value, json};

use crate::{Error, Result, StderrPolicy};

pub(crate) struct Protocol {
    _process: AppServerProcess,
    input: ChildStdin,
    output: Receiver<Result<Value>>,
    next_id: u64,
    timeout: Duration,
    pub(crate) initialize_result: Value,
}

impl Protocol {
    pub(crate) fn spawn(
        codex_path: &PathBuf,
        extra_args: &[OsString],
        timeout: Duration,
        stderr_policy: StderrPolicy,
    ) -> Result<Self> {
        let stderr = match stderr_policy {
            StderrPolicy::Inherit => Stdio::inherit(),
            StderrPolicy::Suppress => Stdio::null(),
        };
        let mut command = Command::new(codex_path);
        command
            .arg("app-server")
            .arg("--stdio")
            .args(extra_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr);
        #[cfg(unix)]
        command.process_group(0);
        let child = command.spawn().map_err(|source| Error::Spawn {
            path: codex_path.clone(),
            source,
        })?;
        let mut process = AppServerProcess::new(child);
        let input = process.child.stdin.take().ok_or_else(|| Error::Protocol {
            message: "Codex App Server has no stdin pipe".to_owned(),
        })?;
        let stdout = process.child.stdout.take().ok_or_else(|| Error::Protocol {
            message: "Codex App Server has no stdout pipe".to_owned(),
        })?;
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let result = line.map_err(Error::Io).and_then(|line| {
                    serde_json::from_str(&line).map_err(|source| Error::InvalidJson {
                        context: "Codex App Server output".to_owned(),
                        source,
                    })
                });
                if sender.send(result).is_err() {
                    return;
                }
            }
        });
        let mut protocol = Self {
            _process: process,
            input,
            output,
            next_id: 1,
            timeout,
            initialize_result: Value::Null,
        };
        let initialize_result = protocol.request(
            "initialize",
            &json!({
                "clientInfo": {
                    "name": "conversations",
                    "title": "Conversations",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {"experimentalApi": true}
            }),
        )?;
        protocol.initialize_result = initialize_result;
        protocol.notify("initialized", &json!({}))?;
        Ok(protocol)
    }

    pub(crate) fn request(&mut self, method: &str, params: &Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or_else(|| Error::Protocol {
            message: "request identifier overflow".to_owned(),
        })?;
        self.send(&json!({"id": id, "method": method, "params": params}))?;
        loop {
            let message =
                self.output
                    .recv_timeout(self.timeout)
                    .map_err(|error| match error {
                        mpsc::RecvTimeoutError::Timeout => Error::Timeout {
                            method: method.to_owned(),
                            seconds: self.timeout.as_secs(),
                        },
                        mpsc::RecvTimeoutError::Disconnected => Error::Protocol {
                            message: format!("Codex App Server closed while waiting for {method}"),
                        },
                    })??;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(Error::Rpc {
                        method: method.to_owned(),
                        code: error.get("code").and_then(Value::as_i64).unwrap_or(-1),
                        message: error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown App Server error")
                            .to_owned(),
                    });
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| Error::Protocol {
                        message: format!("{method} response has neither result nor error"),
                    });
            }
            if message.get("id").is_some() && message.get("method").is_some() {
                self.reject_server_request(&message)?;
            }
        }
    }

    fn notify(&mut self, method: &str, params: &Value) -> Result<()> {
        self.send(&json!({"method": method, "params": params}))
    }

    fn reject_server_request(&mut self, request: &Value) -> Result<()> {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        self.send(&json!({
            "id": id,
            "error": {
                "code": -32601,
                "message": "Conversations does not support server-initiated requests"
            }
        }))
    }

    fn send(&mut self, value: &Value) -> Result<()> {
        serde_json::to_writer(&mut self.input, value).map_err(|source| Error::InvalidJson {
            context: "Codex App Server request".to_owned(),
            source,
        })?;
        self.input.write_all(b"\n").map_err(Error::Io)?;
        self.input.flush().map_err(Error::Io)
    }
}

struct AppServerProcess {
    child: Child,
    #[cfg(unix)]
    process_group_id: u32,
}

impl AppServerProcess {
    fn new(child: Child) -> Self {
        #[cfg(unix)]
        let process_group_id = child.id();
        Self {
            child,
            #[cfg(unix)]
            process_group_id,
        }
    }
}

impl Drop for AppServerProcess {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // `process_group(0)` made the child's PID this private group's ID.
            // Signal before waiting: the unreaped direct child keeps that PID
            // from being reused even when a wrapper exited before its App
            // Server descendant, so this cannot select an unrelated group.
            let _ = Command::new("/bin/kill")
                .args(["-KILL", "--"])
                .arg(format!("-{}", self.process_group_id))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _result = self.child.kill();
        let _result = self.child.wait();
    }
}
