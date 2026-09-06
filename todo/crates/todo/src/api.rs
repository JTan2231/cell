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
}
impl Failure {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
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

/// Decode the existing todo envelope without changing its wire format.
pub fn decode_response<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ClientError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;

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

/// Calls one explicitly selected todo executable; no automatic retries.
#[derive(Debug, Clone)]
pub struct Client {
    executable: PathBuf,
    database: Option<PathBuf>,
    config: Option<PathBuf>,
}
impl Client {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            database: None,
            config: None,
        }
    }
    #[must_use]
    pub fn with_database(mut self, database: impl Into<PathBuf>) -> Self {
        self.database = Some(database.into());
        self
    }
    /// Select the existing Todo configuration file for this invocation.
    #[must_use]
    pub fn with_config(mut self, config: impl Into<PathBuf>) -> Self {
        self.config = Some(config.into());
        self
    }
    fn call<T: serde::de::DeserializeOwned>(
        &self,
        arguments: &[OsString],
        input: Option<&[u8]>,
    ) -> Result<Reply<T>, ClientError> {
        let mut command = std::process::Command::new(&self.executable);
        command.arg("--json");
        if let Some(config) = &self.config {
            command.arg("--config").arg(config);
        }
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

pub use crate::cli::{
    AssessArgs, Command as Request, ConcernAddArgs, ConcernArgs, ConcernAssessArgs, ConcernCommand,
    ConcernListArgs, DesignAcceptArgs, DesignArgs, DesignCommand, DesignCorrectArgs,
    DesignProposeArgs, DesignRejectArgs, EmailCommand, EmailSendArgs, ListArgs, MaintenanceCommand,
    MigrateArgs, NewArgs, NoteAddArgs, NoteCommand, ResearchArgs, RoutingAcceptArgs, RoutingArgs,
    RoutingCommand, RoutingRejectArgs, SearchArgs, SituationArgs, SituationCommand, TodoArgs,
};
pub use crate::db::MigrationOutcome;
pub use crate::email::EmailPreview;
pub use crate::model::{
    ConcernId, DesignId, DesignSummary, InvalidConcernId, InvalidDesignId,
    InvalidRoutingProposalId, InvalidSituationAssessmentId, InvalidTodoId, InvalidWorkingNoteId,
    ModelQuality, RoutingProposalId, SituationAssessmentId, SituationAssessmentSummary, Todo,
    TodoConcern, TodoId, TodoStatus, TodoSummary, TodoView, WorkingNote, WorkingNoteId,
};
pub use crate::reconciliation_store::{
    AssessmentBase, AssessmentReturnView, Concern, ConcernStatus, DesignView, DirectionBoundary,
    DirectionRevision, RoutingDecision, RoutingProposalView, RoutingTargetView,
    SituationAssessmentView,
};

/// One dated finding in a situation assessment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssessmentFindingView {
    #[serde(rename = "ref")]
    pub local_ref: String,
    pub kind: String,
    pub claim: String,
    pub evidence_refs: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JurisdictionAssignmentView {
    pub party: String,
    pub role: String,
    pub responsibility: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JurisdictionView {
    pub key: String,
    pub concern: String,
    pub assignments: Vec<JurisdictionAssignmentView>,
    pub evidence_refs: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectionMappingView {
    pub boundary_ref: String,
    pub disposition: String,
    pub finding_refs: Vec<String>,
    pub explanation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnresolvedAssessmentView {
    #[serde(rename = "ref")]
    pub local_ref: String,
    pub kind: String,
    pub description: String,
    pub materiality: String,
    pub evidence_refs: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesignDropView {
    pub reason: String,
    pub basis_refs: Vec<String>,
    pub dropped_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JurisdictionChangeView {
    pub operation_id: String,
    pub local_ref: String,
    pub key: String,
    pub action: String,
    pub rationale: String,
    pub status: String,
    pub expected_assignments: Vec<JurisdictionAssignmentView>,
    pub proposed_assignments: Vec<JurisdictionAssignmentView>,
    pub basis_refs: Vec<String>,
    pub drop: Option<DesignDropView>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesignClauseView {
    pub operation_id: String,
    pub local_ref: String,
    pub kind: String,
    pub subject: String,
    pub statement: String,
    pub jurisdiction_ref: Option<String>,
    pub status: String,
    pub basis_refs: Vec<String>,
    pub drop: Option<DesignDropView>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesignChoiceView {
    pub operation_id: String,
    pub local_ref: String,
    pub question: String,
    pub why_material: String,
    pub status: String,
    pub basis_refs: Vec<String>,
    pub drop: Option<DesignDropView>,
}

/// Durable product admission status. Admitted synchronous operations retain
/// their guard through runtime settlement and domain result handling.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaintenanceStatus {
    pub protocol_version: u32,
    pub holds: Vec<String>,
    pub drained: bool,
    pub nonterminal_jobs: Option<usize>,
}

/// Existing untagged CLI payloads. Domain projections retain their current shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged, deny_unknown_fields)]
pub enum Data {
    DeploymentCanary {
        canary: DeploymentCanary,
    },
    Maintenance {
        maintenance: MaintenanceStatus,
    },
    Init {
        database: String,
    },
    Migration(MigrationOutcome),
    Todo(TodoView),
    List {
        todos: Vec<TodoSummary>,
    },
    Search {
        query: String,
        todos: Vec<TodoSummary>,
    },
    Captured {
        concern: Concern,
    },
    Concerns {
        concerns: Vec<Concern>,
    },
    ConcernHistory {
        concern: Concern,
        routing: Vec<RoutingProposalView>,
    },
    NewConcern {
        concern: Concern,
        routing: Option<RoutingProposalView>,
    },
    Routing {
        routing: RoutingProposalView,
    },
    RoutingDecision(RoutingDecision),
    Assessment {
        assessment: SituationAssessmentView,
    },
    DesignDecision {
        design: DesignView,
        changed: bool,
    },
    Design {
        design: DesignView,
    },
    Note {
        todo: TodoId,
        working_note: WorkingNote,
    },
    Transition {
        todo: Todo,
        changed: bool,
    },
    EmailPreview(EmailPreview),
    EmailSent {
        email_id: String,
        idempotency_key: String,
        scheduled: bool,
        to: String,
        attention_count: usize,
        pending_concern_count: usize,
        todo_count: usize,
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
    // Keep the exhaustive command-to-argv mapping together.
    #[allow(clippy::too_many_lines)]
    fn execute_input(
        &self,
        request: &Request,
        input: Option<&[u8]>,
    ) -> Result<Reply<Data>, ClientError> {
        let mut args = Vec::<OsString>::new();
        let mut push = |parts: &[&str]| args.extend(parts.iter().map(|part| OsString::from(*part)));
        match request {
            Request::Maintenance(command) => match command {
                MaintenanceCommand::Canary { directory } => {
                    push(&["maintenance", "canary", "--directory"]);
                    args.push(directory.as_os_str().to_owned());
                }
                MaintenanceCommand::Hold { run_id } => push(&["maintenance", "hold", run_id]),
                MaintenanceCommand::Ready { run_id } => push(&["maintenance", "ready", run_id]),
                MaintenanceCommand::Status => push(&["maintenance", "status"]),
                MaintenanceCommand::Release { run_id } => push(&["maintenance", "release", run_id]),
            },
            Request::Init => push(&["init"]),
            Request::Migrate(a) => {
                push(&["migrate", "--backup"]);
                args.push(a.backup.as_os_str().to_owned());
            }
            Request::New(a) => {
                push(&["new", &a.direction, "--source"]);
                args.push(a.source.as_os_str().to_owned());
                research(&mut args, a.quality, a.model.as_deref());
            }
            Request::Concern(c) => match c {
                ConcernCommand::Add(a) => {
                    push(&["concern", "add", &a.direction, "--source"]);
                    args.push(a.source.as_os_str().to_owned());
                }
                ConcernCommand::List(a) => {
                    push(&["concern", "list", "--limit", &a.limit.to_string()]);
                    if a.all {
                        args.push("--all".into());
                    }
                }
                ConcernCommand::Show(a) => push(&["concern", "show", &a.id.to_string()]),
                ConcernCommand::Assess(a) => {
                    push(&["concern", "assess", &a.id.to_string()]);
                    research(&mut args, a.research.quality, a.research.model.as_deref());
                }
            },
            Request::Routing(c) => match c {
                RoutingCommand::Show(a) => push(&["routing", "show", &a.id.to_string()]),
                RoutingCommand::Accept(a) => {
                    push(&["routing", "accept", &a.id.to_string(), "--source"]);
                    args.push(a.source.as_os_str().to_owned());
                }
                RoutingCommand::Reject(a) => {
                    push(&[
                        "routing",
                        "reject",
                        &a.id.to_string(),
                        "--reason",
                        &a.reason,
                        "--source",
                    ]);
                    args.push(a.source.as_os_str().to_owned());
                }
            },
            Request::Assess(a) => {
                push(&["assess", &a.id.to_string()]);
                research(&mut args, a.research.quality, a.research.model.as_deref());
            }
            Request::Situation(SituationCommand::Show(a)) => {
                push(&["situation", "show", &a.id.to_string()]);
            }
            Request::Design(c) => match c {
                DesignCommand::Show(a) => push(&["design", "show", &a.id.to_string()]),
                DesignCommand::Propose(a) => {
                    push(&["design", "propose", &a.todo.to_string()]);
                    research(&mut args, a.research.quality, a.research.model.as_deref());
                }
                DesignCommand::Correct(a) => {
                    push(&["design", "correct", &a.id.to_string(), &a.feedback]);
                    research(&mut args, a.research.quality, a.research.model.as_deref());
                }
                DesignCommand::Accept(a) => {
                    push(&["design", "accept", &a.id.to_string(), "--source"]);
                    args.push(a.source.as_os_str().to_owned());
                }
                DesignCommand::Reject(a) => {
                    push(&[
                        "design",
                        "reject",
                        &a.id.to_string(),
                        "--reason",
                        &a.reason,
                        "--source",
                    ]);
                    args.push(a.source.as_os_str().to_owned());
                }
            },
            Request::List(a) => {
                push(&["list", "--limit", &a.limit.to_string()]);
                if a.all {
                    args.push("--all".into());
                }
            }
            Request::Search(a) => {
                push(&["search", &a.query, "--limit", &a.limit.to_string()]);
                if a.all {
                    args.push("--all".into());
                }
            }
            Request::Show(a) | Request::Done(a) | Request::Reopen(a) => push(&[
                match request {
                    Request::Show(_) => "show",
                    Request::Done(_) => "done",
                    _ => "reopen",
                },
                &a.id.to_string(),
            ]),
            Request::Note(NoteCommand::Add(a)) => {
                push(&["note", "add", &a.id.to_string(), &a.text]);
            }
            Request::Email(EmailCommand::Preview) => push(&["email", "preview"]),
            Request::Email(EmailCommand::Send(a)) => {
                push(&["email", "send"]);
                if a.scheduled {
                    args.push("--scheduled".into());
                }
            }
        }
        self.call(&args, input)
    }
}
fn research(args: &mut Vec<OsString>, quality: Option<ModelQuality>, model: Option<&str>) {
    if let Some(quality) = quality {
        args.extend([
            "--quality".into(),
            match quality {
                ModelQuality::Low => "low",
                ModelQuality::Medium => "medium",
                ModelQuality::High => "high",
            }
            .into(),
        ]);
    }
    if let Some(model) = model {
        args.extend(["--model".into(), model.into()]);
    }
}

/// Immutable managed-tool input values supplied to this provider.
pub mod tools {
    pub use crate::tool_server::contracts::{
        AssessmentDisposition, AssessmentFinding, AssessmentReturn, BoundaryDisposition,
        CandidateReadRequest, ConcernRoutingProposal, DesignChoice, DesignClauseKind, DesignDrop,
        DesignReplacement, DesignRevision, DesignSubmission, DirectionBoundaryAttribution,
        DirectionBoundaryKind, DirectionMapping, DiscardDesignDraft, DraftStatusRequest,
        FindingKind, JurisdictionAction, JurisdictionAssignment, JurisdictionChangeReplacement,
        JurisdictionFinding, JurisdictionRole, NewDesignClause, NewJurisdictionChange, PageRequest,
        ProposedDirection, ProposedDirectionBoundary, RoutingDisposition, RoutingTarget,
        SituationAssessment, SourceReadRequest, SourceSearchRequest, SubjectIdentity, UnifyRoute,
        UnresolvedAssessmentItem, UnresolvedKind,
    };
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeploymentCanary {
    pub protocol_version: u32,
    pub verified: bool,
    pub database: PathBuf,
    pub concern_id: ConcernId,
    pub routing_id: RoutingProposalId,
    pub job_id: String,
}
