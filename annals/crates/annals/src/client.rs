//! Typed transport for the existing Annals command interface.

use std::ffi::{OsStr, OsString};
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command as Process, Stdio};

use clap::ValueEnum;
use serde_json::Value;

use crate::api::{
    AppliedReconciliation, BacklogImportSummary, BackupResult, ChangeCommand, ChildrenResult,
    ConceptCommand, ConceptResult, CorpusOverview, DecisionFeedCommand, DiffView, EnqueueSummary,
    ErrorBody, ErrorOutput, EvidenceResult, GraphView, InboxCommand, InboxRetryCommand,
    InboxRetryWindowArgs, InboxStatus, InitializedLibrary, InterruptSummary, LatelyReport,
    LibraryStats, LogResult, MigratedLibrary, PagedAtArgs, ParentsResult, PauseSummary,
    PrioritySummary, ReconciliationResult, ReconciliationView, RecordedChangeView,
    RegistrationSummary, Request, RetentionResult, RetryEventsResult, RetrySelection, RevertResult,
    RootsResult, RunSummary, SearchOutput, ShakeResult, SuccessEnvelope, ValidatedReconciliation,
    WorkCommand, WorkContent, WorkSummary,
};
use crate::cli::{Command, InstructionsCommand, LibraryCommand};

/// A result selected from the request that was executed. These variants do not
/// introduce a new wire envelope; the CLI continues to emit its existing JSON.
#[derive(Debug)]
pub enum Response {
    Libraries(crate::api::LibraryList),
    LibraryCreated(crate::api::RegisteredLibrary),
    Library(crate::api::NamedLibraryView),
    Instructions(crate::api::InstructionRevision),
    InstructionsSet(crate::api::InstructionSetResult),
    InstructionHistory(crate::api::InstructionHistory),
    Maintenance(crate::maintenance::MaintenanceStatus),
    Initialized(InitializedLibrary),
    Migrated(MigratedLibrary),
    Stats(LibraryStats),
    Overview(CorpusOverview),
    Roots(RootsResult),
    Concept(ConceptResult),
    Parents(ParentsResult),
    Children(ChildrenResult),
    Evidence(EvidenceResult),
    Graph(GraphView),
    Shake(ShakeResult),
    Backup(BackupResult),
    Retained(RetentionResult),
    Works(crate::api::SelectionPage<WorkSummary>),
    Work(WorkContent),
    Reconciliation(ReconciliationResult),
    Applied(AppliedReconciliation),
    Validated(ValidatedReconciliation),
    Reconciliations(crate::api::SelectionPage<ReconciliationView>),
    RecordedChange(RecordedChangeView),
    Search(SearchOutput),
    Lately(LatelyReport),
    Log(LogResult),
    Diff(DiffView),
    Reverted(RevertResult),
    InboxRun(RunSummary),
    Registered(RegistrationSummary),
    Enqueued(EnqueueSummary),
    Accepted(annals_api::AcceptanceReceipt),
    Priority(PrioritySummary),
    BacklogImported(BacklogImportSummary),
    Paused(PauseSummary),
    Interrupted(InterruptSummary),
    RetrySelection(RetrySelection),
    RetryEvent(crate::api::RetryStatus),
    RetryEvents(RetryEventsResult),
    InboxStatus(InboxStatus),
    Watermark(annals_api::Watermark),
    DecisionPage(annals_api::Page),
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("Annals process transport failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Annals returned an invalid response: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Annals command failed with exit code {exit_code:?}: {diagnostics}")]
    Failed {
        exit_code: Option<i32>,
        error: Option<ErrorBody>,
        diagnostics: String,
    },
    #[error("Annals command returned a non-success envelope")]
    InvalidResponse,
    #[error("the selected request value has no CLI representation")]
    InvalidRequest,
    #[error("standard input bytes require a request whose input path is '-'")]
    UnexpectedInput,
}

/// Invokes one explicitly selected Annals executable. Constructing the client
/// has no effects. Calling it performs exactly the selected public command;
/// mutations retain their existing authorization and recovery requirements.
#[derive(Debug, Clone)]
pub struct CliClient {
    pub executable: PathBuf,
    pub library: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub named_library: Option<String>,
    pub expected_library_id: Option<String>,
    pub state_root: Option<PathBuf>,
}

impl CliClient {
    #[must_use]
    pub fn new(executable: PathBuf) -> Self {
        Self {
            executable,
            library: None,
            config: None,
            named_library: None,
            expected_library_id: None,
            state_root: None,
        }
    }

    /// Scope subsequent commands to a registered library name.
    #[must_use]
    pub fn for_named_library(mut self, name: impl Into<String>) -> Self {
        self.named_library = Some(name.into());
        self.library = None;
        self
    }

    /// Require this stable library identity for subsequent commands.
    #[must_use]
    pub fn with_expected_library_id(mut self, library_id: impl Into<String>) -> Self {
        self.expected_library_id = Some(library_id.into());
        self
    }

    /// Execute a typed command using the existing CLI and decode its matching view.
    ///
    /// # Errors
    /// Returns the provider failure or a process/response decoding error.
    pub fn call(&self, request: &Request) -> Result<Response, ClientError> {
        self.call_with_input(request, None)
    }

    /// Execute a command, optionally supplying bytes for its explicit `-` input.
    ///
    /// # Errors
    /// Returns an input mismatch, provider failure, or process/response error.
    pub fn call_with_input(
        &self,
        request: &Request,
        input: Option<&[u8]>,
    ) -> Result<Response, ClientError> {
        if input.is_some() && !reads_stdin(request) {
            return Err(ClientError::UnexpectedInput);
        }
        if self.named_library.is_some()
            && (self.library.is_some() || matches!(request, Command::Library(_) | Command::Init(_)))
        {
            return Err(ClientError::InvalidRequest);
        }
        let mut process = Process::new(&self.executable);
        if let Some(root) = &self.state_root {
            process.env("ANNALS_STATE_DIR", root);
        }
        let mut globals = vec![OsString::from("--json")];
        optional(&mut globals, "--library", self.library.as_deref());
        optional(&mut globals, "--config", self.config.as_deref());
        optional(
            &mut globals,
            "--expected-library-id",
            self.expected_library_id.as_deref(),
        );
        if let Some(name) = &self.named_library {
            globals.push("library".into());
            globals.push(name.into());
        }
        process
            .args(globals)
            .args(arguments(request)?)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = process.spawn()?;
        let written = if let (Some(mut pipe), Some(input)) = (child.stdin.take(), input) {
            pipe.write_all(input)
        } else {
            Ok(())
        };
        let output = child.wait_with_output()?;
        if !output.status.success() {
            let error = serde_json::from_slice::<ErrorOutput>(&output.stderr)
                .ok()
                .map(|v| v.error);
            return Err(ClientError::Failed {
                exit_code: output.status.code(),
                error,
                diagnostics: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        written?;
        let envelope: SuccessEnvelope<Value> = serde_json::from_slice(&output.stdout)?;
        if !envelope.ok {
            return Err(ClientError::InvalidResponse);
        }
        response(request, envelope.data)
    }
}

fn reads_stdin(request: &Request) -> bool {
    if matches!(request, Command::Instructions(InstructionsCommand::Set(args)) if args.stdin) {
        return true;
    }
    let input = match request {
        Command::Work(WorkCommand::Add(args)) => Some(&args.input),
        Command::Integrate(args) => args.input.as_ref(),
        Command::Change(ChangeCommand::Submit(args)) => Some(&args.input),
        _ => None,
    };
    input.is_some_and(|path| path == std::path::Path::new("-"))
}

fn option(arguments: &mut Vec<OsString>, name: &str, value: impl AsRef<OsStr>) {
    let mut argument = OsString::from(name);
    argument.push("=");
    argument.push(value);
    arguments.push(argument);
}

fn optional<T: AsRef<OsStr>>(arguments: &mut Vec<OsString>, name: &str, value: Option<T>) {
    if let Some(value) = value {
        option(arguments, name, value);
    }
}

fn number(arguments: &mut Vec<OsString>, name: &str, value: impl ToString + Copy) {
    option(arguments, name, value.to_string());
}

fn enumeration(
    arguments: &mut Vec<OsString>,
    name: &str,
    value: impl ValueEnum + Copy,
) -> Result<(), ClientError> {
    let value = value
        .to_possible_value()
        .ok_or(ClientError::InvalidRequest)?;
    option(arguments, name, value.get_name());
    Ok(())
}

fn page(arguments: &mut Vec<OsString>, value: &PagedAtArgs) {
    if let Some(at) = value.at {
        number(arguments, "--at", at);
    }
    number(arguments, "--limit", value.limit);
    optional(arguments, "--cursor", value.cursor.as_deref());
}

fn positional(arguments: &mut Vec<OsString>, values: impl IntoIterator<Item = OsString>) {
    arguments.push("--".into());
    arguments.extend(values);
}

fn retry_window(arguments: &mut Vec<OsString>, value: &InboxRetryWindowArgs) {
    option(arguments, "--from", &value.from_job_id);
    option(arguments, "--through", &value.through_job_id);
}

#[allow(clippy::too_many_lines)]
fn arguments(request: &Request) -> Result<Vec<OsString>, ClientError> {
    let mut a = Vec::new();
    match request {
        Command::Library(command) => {
            a.push("library".into());
            match command {
                LibraryCommand::List => a.push("list".into()),
                LibraryCommand::Create(args) => {
                    a.push("create".into());
                    enumeration(&mut a, "--kind", args.kind)?;
                    positional(&mut a, [args.name.clone().into()]);
                }
                LibraryCommand::Named(_) => return Err(ClientError::InvalidRequest),
            }
        }
        Command::Show => a.push("show".into()),
        Command::Instructions(command) => {
            a.push("instructions".into());
            match command {
                InstructionsCommand::Show => a.push("show".into()),
                InstructionsCommand::History(args) => {
                    a.push("history".into());
                    number(&mut a, "--limit", args.limit);
                    if let Some(before) = args.before {
                        number(&mut a, "--before", before);
                    }
                }
                InstructionsCommand::Set(args) => {
                    a.push("set".into());
                    optional(&mut a, "--file", args.file.as_deref());
                    if args.stdin {
                        a.push("--stdin".into());
                    }
                    if let Some(content) = &args.content {
                        positional(&mut a, [content.into()]);
                    }
                }
            }
        }
        Command::Maintenance(command) => {
            a.push("maintenance".into());
            match command {
                crate::maintenance::MaintenanceCommand::Status => a.push("status".into()),
                crate::maintenance::MaintenanceCommand::Hold { run_id } => {
                    a.push("hold".into());
                    a.push(run_id.into());
                }
                crate::maintenance::MaintenanceCommand::Release { run_id } => {
                    a.push("release".into());
                    a.push(run_id.into());
                }
            }
        }
        Command::Init(v) => {
            a.push("init".into());
            enumeration(&mut a, "--kind", v.kind)?;
        }
        Command::Migrate => a.push("migrate".into()),
        Command::Stats => a.push("stats".into()),
        Command::Overview(v) => {
            a.push("overview".into());
            if let Some(at) = v.at {
                number(&mut a, "--at", at);
            }
        }
        Command::Roots(v) => {
            a.push("roots".into());
            page(&mut a, v);
        }
        Command::Concept(v) => {
            a.push("concept".into());
            match v {
                ConceptCommand::Show(v) => {
                    a.push("show".into());
                    if let Some(at) = v.at {
                        number(&mut a, "--at", at);
                    }
                    number(&mut a, "--preview-limit", v.preview_limit);
                    positional(&mut a, [v.id.to_string().into()]);
                }
                ConceptCommand::Parents(v)
                | ConceptCommand::Children(v)
                | ConceptCommand::Evidence(v) => {
                    a.push(
                        match request {
                            Command::Concept(ConceptCommand::Parents(_)) => "parents",
                            Command::Concept(ConceptCommand::Children(_)) => "children",
                            _ => "evidence",
                        }
                        .into(),
                    );
                    page(&mut a, &v.page);
                    positional(&mut a, [v.id.to_string().into()]);
                }
            }
        }
        Command::Graph(v) => {
            a.push("graph".into());
            if let Some(at) = v.at {
                number(&mut a, "--at", at);
            }
            enumeration(&mut a, "--direction", v.direction)?;
            number(&mut a, "--depth", v.depth);
            number(&mut a, "--max-nodes", v.max_nodes);
            positional(&mut a, [v.id.to_string().into()]);
        }
        Command::Shake(v) => {
            a.push("shake".into());
            if v.yes {
                a.push("--yes".into());
            }
        }
        Command::Backup(v) => {
            a.push("backup".into());
            positional(&mut a, [v.output.clone().into_os_string()]);
        }
        Command::Work(v) => {
            a.push("work".into());
            match v {
                WorkCommand::List { limit } => {
                    a.push("list".into());
                    number(&mut a, "--limit", limit);
                }
                WorkCommand::Show(v) => {
                    a.push("show".into());
                    positional(&mut a, [v.label.clone().into()]);
                }
                WorkCommand::Add(v) => {
                    a.push("add".into());
                    optional(&mut a, "--name", v.name.as_deref());
                    positional(&mut a, [v.input.clone().into_os_string()]);
                }
            }
        }
        Command::Integrate(v) => {
            a.push("integrate".into());
            optional(&mut a, "--work", v.work.as_deref());
            optional(&mut a, "--name", v.name.as_deref());
            if let Some(quality) = v.quality {
                enumeration(&mut a, "--quality", quality)?;
            }
            optional(&mut a, "--model", v.model.as_deref());
            if v.reexamine {
                a.push("--reexamine".into());
            }
            if v.apply {
                a.push("--apply".into());
            }
            if let Some(input) = &v.input {
                positional(&mut a, [input.clone().into_os_string()]);
            }
        }
        Command::Inbox(v) => inbox_arguments(&mut a, v)?,
        Command::DecisionFeed(v) => {
            a.push("decision-feed".into());
            match v {
                DecisionFeedCommand::Watermark => a.push("watermark".into()),
                DecisionFeedCommand::Page(v) => {
                    a.push("page".into());
                    option(&mut a, "--watermark", &v.watermark);
                    option(&mut a, "--after", &v.after);
                    number(&mut a, "--limit", v.limit);
                }
            }
        }
        Command::Change(v) => {
            a.push("change".into());
            match v {
                ChangeCommand::List { limit } => {
                    a.push("list".into());
                    number(&mut a, "--limit", limit);
                }
                ChangeCommand::Submit(v) => {
                    a.push("submit".into());
                    option(&mut a, "--work", &v.work);
                    number(&mut a, "--base", v.base);
                    positional(&mut a, [v.input.clone().into_os_string()]);
                }
                ChangeCommand::Show(v) => {
                    a.push("show".into());
                    optional(&mut a, "--work", v.work.as_deref());
                    if let Some(at) = v.at {
                        number(&mut a, "--at", at);
                    }
                }
                ChangeCommand::Validate(v) | ChangeCommand::Apply(v) => {
                    a.push(
                        if matches!(request, Command::Change(ChangeCommand::Validate(_))) {
                            "validate"
                        } else {
                            "apply"
                        }
                        .into(),
                    );
                    optional(&mut a, "--work", v.work.as_deref());
                }
            }
        }
        Command::Search(v) => {
            a.push("search".into());
            if let Some(at) = v.at {
                number(&mut a, "--at", at);
            }
            if let Some(within) = v.within {
                option(&mut a, "--within", within.to_string());
            }
            number(&mut a, "--limit", v.limit);
            optional(&mut a, "--cursor", v.cursor.as_deref());
            positional(&mut a, [v.query.clone().into()]);
        }
        Command::Lately(v) => {
            a.push("lately".into());
            option(&mut a, "--since", &v.since);
            optional(&mut a, "--until", v.until.as_deref());
            enumeration(&mut a, "--by", v.by)?;
            if let Some(status) = v.status {
                enumeration(&mut a, "--status", status)?;
            }
            if let Some(channel) = v.channel {
                enumeration(&mut a, "--channel", channel)?;
            }
        }
        Command::Log(v) => {
            a.push("log".into());
            number(&mut a, "--limit", v.limit);
        }
        Command::Diff(v) => {
            a.push("diff".into());
            positional(&mut a, [v.from.to_string().into(), v.to.to_string().into()]);
        }
        Command::Revert(v) => {
            a.push("revert".into());
            positional(&mut a, [v.revision.to_string().into()]);
        }
    }
    Ok(a)
}

fn inbox_arguments(a: &mut Vec<OsString>, request: &InboxCommand) -> Result<(), ClientError> {
    a.push("inbox".into());
    match request {
        InboxCommand::Run(v) | InboxCommand::Register(v) => {
            a.push(
                if matches!(request, InboxCommand::Run(_)) {
                    "run"
                } else {
                    "register"
                }
                .into(),
            );
            if let Some(seconds) = v.settle_seconds {
                number(a, "--settle-seconds", seconds);
            }
            if v.stop_on_failure {
                a.push("--stop-on-failure".into());
            }
        }
        InboxCommand::Enqueue(v) => {
            a.push("enqueue".into());
            if v.priority {
                a.push("--priority".into());
            }
            positional(a, v.inputs.iter().map(|p| p.as_os_str().to_owned()));
        }
        InboxCommand::Accept(v) => {
            a.push("accept".into());
            option(a, "--producer", &v.producer);
            option(a, "--key", &v.key);
            positional(a, [v.input.clone().into_os_string()]);
        }
        InboxCommand::Prioritize(v) | InboxCommand::Deprioritize(v) => {
            a.push(
                if matches!(request, InboxCommand::Prioritize(_)) {
                    "prioritize"
                } else {
                    "deprioritize"
                }
                .into(),
            );
            positional(a, v.job_ids.iter().map(OsString::from));
        }
        InboxCommand::ImportBacklog(v) => {
            a.push("import-backlog".into());
            option(a, "--from", &v.from);
        }
        InboxCommand::Pause => a.push("pause".into()),
        InboxCommand::Resume => a.push("resume".into()),
        InboxCommand::Status => a.push("status".into()),
        InboxCommand::Interrupt(v) => {
            a.push("interrupt".into());
            enumeration(a, "--as", v.disposition)?;
            optional(a, "--reason", v.reason.as_deref());
            positional(a, [v.job_id.clone().into()]);
        }
        InboxCommand::Retry(v) => {
            a.push("retry".into());
            match v {
                InboxRetryCommand::Preview(v) => {
                    a.push("preview".into());
                    retry_window(a, v);
                }
                InboxRetryCommand::Start(v) => {
                    a.push("start".into());
                    retry_window(a, &v.window);
                    optional(a, "--reason", v.reason.as_deref());
                }
                InboxRetryCommand::Status(v) => {
                    a.push("status".into());
                    if v.details {
                        a.push("--details".into());
                    }
                    number(a, "--limit", v.limit);
                    if let Some(id) = v.event_id {
                        positional(a, [id.to_string().into()]);
                    }
                }
                InboxRetryCommand::Continue(v) => {
                    a.push("continue".into());
                    positional(a, [v.event_id.to_string().into()]);
                }
            }
        }
    }
    Ok(())
}

fn response(request: &Request, data: Value) -> Result<Response, ClientError> {
    macro_rules! decode {
        ($variant:ident) => {
            serde_json::from_value(data)
                .map(Response::$variant)
                .map_err(Into::into)
        };
    }
    match request {
        Command::Library(command) => match command {
            LibraryCommand::List => decode!(Libraries),
            LibraryCommand::Create(_) => decode!(LibraryCreated),
            LibraryCommand::Named(_) => Err(ClientError::InvalidRequest),
        },
        Command::Show => decode!(Library),
        Command::Instructions(command) => match command {
            InstructionsCommand::Show => decode!(Instructions),
            InstructionsCommand::Set(_) => decode!(InstructionsSet),
            InstructionsCommand::History(_) => decode!(InstructionHistory),
        },
        Command::Maintenance(_) => decode!(Maintenance),
        Command::Init(_) => decode!(Initialized),
        Command::Migrate => decode!(Migrated),
        Command::Stats => decode!(Stats),
        Command::Overview(_) => decode!(Overview),
        Command::Roots(_) => decode!(Roots),
        Command::Graph(_) => decode!(Graph),
        Command::Shake(_) => decode!(Shake),
        Command::Backup(_) => decode!(Backup),
        Command::Concept(v) => match v {
            ConceptCommand::Show(_) => decode!(Concept),
            ConceptCommand::Parents(_) => decode!(Parents),
            ConceptCommand::Children(_) => decode!(Children),
            ConceptCommand::Evidence(_) => decode!(Evidence),
        },
        Command::Work(v) => match v {
            WorkCommand::Add(_) => decode!(Retained),
            WorkCommand::List { .. } => decode!(Works),
            WorkCommand::Show(_) => decode!(Work),
        },
        Command::Integrate(_) => {
            if data.get("revision").is_some() {
                decode!(Applied)
            } else {
                decode!(Reconciliation)
            }
        }
        Command::Change(v) => match v {
            ChangeCommand::Show(v) if v.at.is_some() => decode!(RecordedChange),
            ChangeCommand::Submit(_) | ChangeCommand::Show(_) => decode!(Reconciliation),
            ChangeCommand::Validate(_) => decode!(Validated),
            ChangeCommand::Apply(_) => decode!(Applied),
            ChangeCommand::List { .. } => decode!(Reconciliations),
        },
        Command::Search(_) => decode!(Search),
        Command::Lately(_) => decode!(Lately),
        Command::Log(_) => decode!(Log),
        Command::Diff(_) => decode!(Diff),
        Command::Revert(_) => decode!(Reverted),
        Command::DecisionFeed(v) => match v {
            DecisionFeedCommand::Watermark => decode!(Watermark),
            DecisionFeedCommand::Page(_) => decode!(DecisionPage),
        },
        Command::Inbox(v) => match v {
            InboxCommand::Run(_) => decode!(InboxRun),
            InboxCommand::Register(_) => decode!(Registered),
            InboxCommand::Enqueue(_) => decode!(Enqueued),
            InboxCommand::Accept(_) => decode!(Accepted),
            InboxCommand::Prioritize(_) | InboxCommand::Deprioritize(_) => decode!(Priority),
            InboxCommand::ImportBacklog(_) => decode!(BacklogImported),
            InboxCommand::Pause | InboxCommand::Resume => decode!(Paused),
            InboxCommand::Interrupt(_) => decode!(Interrupted),
            InboxCommand::Status => decode!(InboxStatus),
            InboxCommand::Retry(v) => match v {
                InboxRetryCommand::Preview(_) => decode!(RetrySelection),
                InboxRetryCommand::Status(v) if v.event_id.is_none() => decode!(RetryEvents),
                _ => decode!(RetryEvent),
            },
        },
    }
}
