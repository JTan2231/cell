//! Provider-owned public values and local CLI transport. Import these types at the
//! boundary, then convert them into application-owned values where needed.
#![allow(
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value
)]

use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Success<T> {
    pub schema_version: u32,
    pub ok: bool,
    pub data: T,
}
impl<T> Success<T> {
    pub fn new(data: T) -> Self {
        Self {
            schema_version: 1,
            ok: true,
            data,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Failure {
    pub schema_version: u32,
    pub ok: bool,
    pub error: ErrorBody,
}
impl Failure {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            ok: false,
            error: ErrorBody {
                code: code.into(),
                message: message.into(),
            },
        }
    }
}

#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Protocol(String),
    Rejected(Failure),
}
impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Json(e) => e.fmt(f),
            Self::Protocol(e) => e.fmt(f),
            Self::Rejected(e) => write!(f, "{}: {}", e.error.code, e.error.message),
        }
    }
}
impl std::error::Error for ClientError {}
impl From<std::io::Error> for ClientError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<serde_json::Error> for ClientError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

/// Decode the existing geste envelope without changing its wire format.
pub fn decode_response<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ClientError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    if value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return Err(ClientError::Protocol("unsupported response schema".into()));
    }
    match value.get("ok").and_then(serde_json::Value::as_bool) {
        Some(true) if value.get("error").is_none() => {
            Ok(serde_json::from_value::<Success<T>>(value)?.data)
        }
        Some(false) if value.get("data").is_none() => {
            Err(ClientError::Rejected(serde_json::from_value(value)?))
        }
        _ => Err(ClientError::Protocol("invalid response envelope".into())),
    }
}

/// A decoded result plus diagnostics emitted after a durable domain result.
#[derive(Debug)]
pub struct Reply<T> {
    pub data: T,
    pub diagnostics: String,
}

/// Calls one explicitly selected geste executable; no automatic retries.
#[derive(Debug, Clone)]
pub struct Client {
    executable: PathBuf,
    database: Option<PathBuf>,
}
impl Client {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            database: None,
        }
    }
    #[must_use]
    pub fn with_database(mut self, database: impl Into<PathBuf>) -> Self {
        self.database = Some(database.into());
        self
    }
    fn call<T: serde::de::DeserializeOwned>(
        &self,
        arguments: &[OsString],
        input: Option<&[u8]>,
    ) -> Result<Reply<T>, ClientError> {
        let mut command = std::process::Command::new(&self.executable);
        command.arg("--json");
        if let Some(database) = &self.database {
            command.arg("--database").arg(database);
        }
        command
            .args(arguments)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        command.stdin(if input.is_some() {
            std::process::Stdio::piped()
        } else {
            std::process::Stdio::null()
        });
        let mut child = command.spawn()?;
        if let Some(input) = input {
            use std::io::Write as _;
            if let Some(mut stdin) = child.stdin.take()
                && let Err(error) = stdin.write_all(input)
            {
                drop(stdin);
                let _ = child.wait();
                return Err(ClientError::Io(error));
            }
        }
        let output = child.wait_with_output()?;
        let bytes = if output.status.success() {
            &output.stdout
        } else {
            &output.stderr
        };
        let data = decode_response(bytes)?;
        if !output.status.success() {
            return Err(ClientError::Protocol(
                "success envelope with unsuccessful process status".into(),
            ));
        }
        Ok(Reply {
            data,
            diagnostics: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

pub use crate::cli::{Command as Request, EpisodeCommand, ReadArgs, SearchArgs};
pub use crate::error::AppError as CaptureError;
pub use crate::model::{
    Capture, EpisodeListItem, EpisodeRelation, Graph, GraphEdge, GraphNode, GraphSource, Outcome,
    OutcomeStatus, RelatedEpisode, Report, RevisionView, SearchResult, Settlement,
    SettlementStatus, SourceAnchor, SourceRole,
};

/// Parse and validate the same strict bounded capture input accepted by the CLI.
pub fn decode_capture(bytes: &[u8]) -> Result<Capture, CaptureError> {
    crate::capture::parse_capture(bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Data {
    Init {
        database: PathBuf,
        schema_version: i64,
        created: bool,
    },
    Doctor {
        database: PathBuf,
        schema_version: i64,
        foreign_keys: String,
        integrity: String,
        permissions: String,
    },
    SearchResults {
        query_terms: Vec<String>,
        results: Vec<SearchResult>,
    },
    EpisodeCreated {
        episode: RevisionView,
    },
    EpisodeRevised {
        episode: RevisionView,
    },
    EpisodeList {
        episodes: Vec<EpisodeListItem>,
    },
    EpisodeRevision {
        episode: RevisionView,
    },
    EpisodeReport {
        episode: RevisionView,
        interpretation_label: String,
        source_boundary: String,
        warnings: Vec<String>,
    },
    EpisodeGraph {
        episode: String,
        revision: u32,
        interpretation_label: String,
        source_boundary: String,
        nodes: Vec<GraphNode>,
        edges: Vec<GraphEdge>,
        warnings: Vec<String>,
    },
}
impl Client {
    pub fn execute(&self, request: &Request) -> Result<Reply<Data>, ClientError> {
        self.execute_input(request, None)
    }
    /// Supply exact standard-input bytes for a request whose document is `-`.
    pub fn execute_with_input(
        &self,
        request: &Request,
        input: &[u8],
    ) -> Result<Reply<Data>, ClientError> {
        self.execute_input(request, Some(input))
    }
    fn execute_input(
        &self,
        request: &Request,
        input: Option<&[u8]>,
    ) -> Result<Reply<Data>, ClientError> {
        let mut args: Vec<OsString> = Vec::new();
        let mut push = |parts: &[&str]| args.extend(parts.iter().map(|part| OsString::from(*part)));
        match request {
            Request::Init => push(&["init"]),
            Request::Doctor => push(&["doctor"]),
            Request::Search(a) => push(&["search", &a.query, "--limit", &a.limit.to_string()]),
            Request::Episode { command } => match command {
                EpisodeCommand::Create { input } => {
                    push(&["episode", "create"]);
                    args.push(input.as_os_str().to_owned());
                }
                EpisodeCommand::Revise {
                    episode,
                    input,
                    base,
                } => {
                    push(&["episode", "revise", episode]);
                    args.push(input.as_os_str().to_owned());
                    args.extend(["--base".into(), base.to_string().into()]);
                }
                EpisodeCommand::List { limit } => {
                    push(&["episode", "list", "--limit", &limit.to_string()]);
                }
                EpisodeCommand::Show(a) => {
                    push(&["episode", "show", &a.episode]);
                    if let Some(at) = a.at {
                        args.extend(["--at".into(), at.to_string().into()]);
                    }
                }
            },
            Request::Report(a) | Request::Graph(a) => {
                push(&[
                    if matches!(request, Request::Report(_)) {
                        "report"
                    } else {
                        "graph"
                    },
                    &a.episode,
                ]);
                if let Some(at) = a.at {
                    args.extend(["--at".into(), at.to_string().into()]);
                }
            }
        }
        self.call(&args, input)
    }
}
