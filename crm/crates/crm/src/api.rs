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
    pub ok: bool,
    pub data: T,
}
impl<T> Success<T> {
    pub fn new(data: T) -> Self {
        Self { ok: true, data }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Failure {
    pub ok: bool,
    pub error: ErrorBody,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<UpdateData>,
}
impl Failure {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: ErrorBody {
                code: code.into(),
                message: message.into(),
            },
            context: None,
        }
    }
}

#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Protocol(String),
    Rejected(Box<Failure>),
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

/// Decode the existing crm envelope without changing its wire format.
pub fn decode_response<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ClientError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;

    match value.get("ok").and_then(serde_json::Value::as_bool) {
        Some(true) if value.get("error").is_none() => {
            Ok(serde_json::from_value::<Success<T>>(value)?.data)
        }
        Some(false) if value.get("data").is_none() => Err(ClientError::Rejected(Box::new(
            serde_json::from_value(value)?,
        ))),
        _ => Err(ClientError::Protocol("invalid response envelope".into())),
    }
}

/// A decoded result plus diagnostics emitted after a durable domain result.
#[derive(Debug)]
pub struct Reply<T> {
    pub data: T,
    pub diagnostics: String,
}

/// Calls one explicitly selected crm executable; no automatic retries.
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

pub use crate::model::{
    CaseListItem, CaseRevision, ProfileEntry, RevisionProposal, SearchResult, Stage, StewardUpdate,
    UpdateStatus,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateView {
    #[serde(flatten)]
    pub update: StewardUpdate,
    pub advisory: Option<String>,
    pub attention: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateData {
    #[serde(rename = "type")]
    pub kind: String,
    pub update: UpdateView,
}

/// Every public CLI success payload. Hidden workers have no public response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Data {
    Init {
        database: PathBuf,
        schema_version: u32,
        created: bool,
    },
    Migrated {
        database: PathBuf,
        backup: Option<PathBuf>,
        from_schema_version: u32,
        schema_version: u32,
        changed: bool,
    },
    ProfileEntry {
        entry: ProfileEntry,
    },
    ProfileList {
        entries: Vec<ProfileEntry>,
    },
    Doctor {
        database: PathBuf,
        schema_version: u32,
        foreign_keys: String,
        integrity: String,
        nucleus: String,
    },
    CaseCreated {
        case: CaseRevision,
    },
    CaseList {
        cases: Vec<CaseListItem>,
    },
    CaseRevision {
        case: CaseRevision,
    },
    CaseHistory {
        revisions: Vec<CaseRevision>,
    },
    SearchResults {
        results: Vec<SearchResult>,
    },
    UpdateQueued {
        update: UpdateView,
        activation_warning: Option<String>,
    },
    UpdateRetried {
        update: UpdateView,
        activation_warning: Option<String>,
    },
    UpdateList {
        updates: Vec<UpdateView>,
    },
    Update {
        update: UpdateView,
    },
}

/// Public commands; `_worker` remains private to CRM.
#[derive(Debug)]
pub enum Request {
    Init,
    Migrate {
        backup: PathBuf,
    },
    Doctor,
    CreateProfileEntry {
        title: String,
        input: PathBuf,
    },
    ListProfileEntries {
        limit: usize,
    },
    ShowProfileEntry {
        entry: String,
    },
    UpdateProfileEntry {
        entry: String,
        title: String,
        input: PathBuf,
    },
    CreateCase {
        title: String,
        input: Option<PathBuf>,
        stage: Stage,
    },
    ListCases {
        limit: usize,
    },
    ShowCase {
        case: String,
        revision: Option<u64>,
    },
    CaseHistory {
        case: String,
    },
    Search {
        query: String,
        limit: usize,
    },
    Tell {
        case: String,
        input: PathBuf,
        name: Option<String>,
        source: Option<String>,
    },
    ListUpdates {
        limit: usize,
    },
    ShowUpdate {
        update: String,
    },
    WaitUpdate {
        update: String,
        timeout_seconds: u64,
    },
    ResumeUpdate {
        update: String,
    },
    RetryUpdate {
        update: String,
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
            Request::Migrate { backup } => {
                push(&["migrate", "--backup"]);
                args.push(backup.as_os_str().to_owned());
            }
            Request::Doctor => push(&["doctor"]),
            Request::CreateProfileEntry { title, input } => {
                push(&["profile", "new", "--title", title]);
                args.push(input.as_os_str().to_owned());
            }
            Request::ListProfileEntries { limit } => {
                push(&["profile", "list", "--limit", &limit.to_string()]);
            }
            Request::ShowProfileEntry { entry } => push(&["profile", "show", entry]),
            Request::UpdateProfileEntry {
                entry,
                title,
                input,
            } => {
                push(&["profile", "update", entry, "--title", title]);
                args.push(input.as_os_str().to_owned());
            }
            Request::CreateCase {
                title,
                input,
                stage,
            } => {
                push(&["case", "new", "--title", title, "--stage", stage.as_str()]);
                if let Some(path) = input {
                    args.push(path.as_os_str().to_owned());
                }
            }
            Request::ListCases { limit } => push(&["case", "list", "--limit", &limit.to_string()]),
            Request::ShowCase { case, revision } => {
                push(&["case", "show", case]);
                if let Some(revision) = revision {
                    args.extend(["--revision".into(), revision.to_string().into()]);
                }
            }
            Request::CaseHistory { case } => push(&["case", "history", case]),
            Request::Search { query, limit } => {
                push(&["search", query, "--limit", &limit.to_string()]);
            }
            Request::Tell {
                case,
                input,
                name,
                source,
            } => {
                push(&["tell", case]);
                args.push(input.as_os_str().to_owned());
                for (flag, value) in [("--name", name), ("--source", source)] {
                    if let Some(value) = value {
                        args.extend([flag.into(), value.into()]);
                    }
                }
            }
            Request::ListUpdates { limit } => {
                push(&["update", "list", "--limit", &limit.to_string()]);
            }
            Request::ShowUpdate { update } => push(&["update", "show", update]),
            Request::WaitUpdate {
                update,
                timeout_seconds,
            } => push(&[
                "update",
                "wait",
                update,
                "--timeout",
                &timeout_seconds.to_string(),
            ]),
            Request::ResumeUpdate { update } => push(&["update", "resume", update]),
            Request::RetryUpdate { update } => push(&["update", "retry", update]),
        }
        self.call(&args, input)
    }
}
