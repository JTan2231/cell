//! Shared document construction used by the observer and local rendering commands.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Subcommand;
use conversations::{AppServerClient, ClientConfig, StderrPolicy};
use decisions::document::{Classification, Snapshot, classification_schema};
use fs2::FileExt as _;
use nucleus_client::NucleusClient;
use nucleus_core::{
    AbsolutePath, AgentInvocationV1, BuiltinToolsV1, JobId, JobRequestV1, LogSchemaV1, ModelId,
    PROTOCOL_VERSION_V1, ReasoningEffort, Requester, SchemaId, TimeoutSeconds, ToolCallV1,
    ToolCallsQueryV1, ToolDefinitionV1, ToolResultV1, ToolsetDefinitionsV1, ToolsetRef,
    ToolsetRegistrationV1, WorkspaceAccess,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::value::to_raw_value;
use sha2::{Digest as _, Sha256};

use crate::classifier::{classifier_cwd, require_health};
use crate::error::{AppError, AppResult, Context as _};

const INPUT_SCHEMA: &str = "krisis.tool.submit-decision.input.v1";
const RESULT_SCHEMA: &str = "krisis.tool.submit-decision.result.v1";
const TOOL_NAME: &str = "submit_decision";

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Freeze full history and classify one completed exchange, or resume the same run.
    Build {
        #[arg(long)]
        thread_id: String,
        #[arg(long)]
        turn_id: String,
        /// Private directory for the frozen run and optional decision.md.
        #[arg(long)]
        directory: PathBuf,
    },
    /// Recreate decision.md from a saved classification without source or Nucleus access.
    Render {
        #[arg(long)]
        directory: PathBuf,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    call_digest: String,
    result: ToolResultV1,
    classification: Option<Classification>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Run {
    version: u32,
    snapshot: Snapshot,
    request: JobRequestV1,
    receipts: Vec<Receipt>,
    terminal: bool,
}

impl Run {
    fn classification(&self) -> Option<&Classification> {
        self.receipts
            .iter()
            .find_map(|receipt| receipt.classification.as_ref())
    }

    fn read(directory: &Path) -> AppResult<Self> {
        let bytes = fs::read(directory.join("run.json"))
            .context("document_run_unavailable", "cannot read document run")?;
        let run: Self = serde_json::from_slice(&bytes)
            .context("document_run_invalid", "cannot decode document run")?;
        run.snapshot
            .validate()
            .context("document_source_invalid", "invalid frozen source")?;
        if run.version != 1
            || run.request.prompt
                != run
                    .snapshot
                    .prompt()
                    .context("document_source_invalid", "invalid frozen prompt")?
        {
            return Err(AppError::new(
                "document_run_invalid",
                "run version or source/request binding differs",
            ));
        }
        if let Some(classification) = run.classification() {
            classification
                .validate()
                .context("document_run_invalid", "invalid saved classification")?;
        }
        Ok(run)
    }

    fn save(&self, directory: &Path) -> AppResult<()> {
        let bytes = serde_json::to_vec_pretty(self)
            .context("document_run_invalid", "cannot encode document run")?;
        atomic_write(directory, "run.json", &bytes)
    }

    fn render(&self, directory: &Path) -> AppResult<Option<PathBuf>> {
        let classification = self.classification().ok_or_else(|| {
            AppError::new(
                "document_classification_missing",
                "run has no accepted classification",
            )
        })?;
        let markdown = self
            .snapshot
            .render(classification)
            .context("document_render_failed", "cannot render document")?;
        let path = directory.join("decision.md");
        let Some(markdown) = markdown else {
            if path.exists() {
                return Err(AppError::new(
                    "document_conflict",
                    "negative classification has an unexpected decision.md",
                ));
            }
            return Ok(None);
        };
        match fs::read(&path) {
            Ok(bytes) if bytes == markdown.as_bytes() => {}
            Ok(_) => {
                return Err(AppError::new(
                    "document_conflict",
                    "existing decision.md differs from the frozen result",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                atomic_write(directory, "decision.md", markdown.as_bytes())?;
            }
            Err(error) => return Err(AppError::new("document_read_failed", error.to_string())),
        }
        Ok(Some(path))
    }

    /// Save the exact response and accepted domain result together before any acknowledgement.
    fn accept_call(&mut self, directory: &Path, call: &ToolCallV1) -> AppResult<ToolResultV1> {
        if call.job_id != self.request.id || call.version != PROTOCOL_VERSION_V1 {
            return Err(AppError::new(
                "document_call_invalid",
                "tool call has a different job or protocol",
            ));
        }
        let bytes =
            serde_json::to_vec(call).context("document_call_invalid", "cannot encode call")?;
        let call_digest = format!("{:x}", Sha256::digest(&bytes));
        if let Some(receipt) = self
            .receipts
            .iter()
            .find(|receipt| receipt.result.call_id == call.id)
        {
            if receipt.call_digest != call_digest {
                return Err(AppError::new(
                    "document_call_conflict",
                    "replayed call content differs",
                ));
            }
            return Ok(receipt.result.clone());
        }
        let parsed =
            if call.tool_name != TOOL_NAME || call.arguments_schema_id.as_str() != INPUT_SCHEMA {
                Err("expected submit_decision with its version 1 input schema".to_owned())
            } else if self.classification().is_some() {
                Err("this run already has an accepted classification".to_owned())
            } else {
                Classification::parse(call.arguments.get()).map_err(|error| error.to_string())
            };
        let (classification, value, is_error) = match parsed {
            Ok(classification) => (Some(classification), json!({"accepted": true}), false),
            Err(message) => (
                None,
                json!({"error": {"code": "invalid_structure", "message": message}}),
                true,
            ),
        };
        let result = ToolResultV1 {
            version: PROTOCOL_VERSION_V1,
            call_id: call.id.clone(),
            requester: self.request.requester.clone(),
            result_schema_id: SchemaId::new(RESULT_SCHEMA),
            result: to_raw_value(&value)
                .context("document_result_invalid", "cannot encode tool result")?,
            is_error,
        };
        self.receipts.push(Receipt {
            call_digest,
            result: result.clone(),
            classification,
        });
        self.save(directory)?;
        Ok(result)
    }
}

pub(crate) struct DocumentOutput {
    pub snapshot: Snapshot,
    pub classification: Classification,
    pub document: Option<PathBuf>,
    pub job_id: JobId,
    pub run_path: PathBuf,
}

/// Uncertain jobs and accepted results must resume their exact saved request.
pub(crate) fn retry_starts_new_attempt(directory: &Path) -> AppResult<bool> {
    if !directory
        .join("run.json")
        .try_exists()
        .context("document_run_unavailable", "cannot inspect saved run")?
    {
        return Ok(true);
    }
    let run = Run::read(directory)?;
    Ok(run.terminal && run.classification().is_none())
}

pub(crate) fn run(command: Command) -> AppResult<()> {
    let output = build(command, |_| Ok(()))?;
    crate::print_json(&json!({
        "job_id": output.job_id, "classification": output.classification,
        "document": output.document, "run": output.run_path,
    }))
}

pub(crate) fn build(
    command: Command,
    before_classify: impl FnOnce(&Snapshot) -> AppResult<()>,
) -> AppResult<DocumentOutput> {
    let directory = match &command {
        Command::Build { directory, .. } | Command::Render { directory } => directory,
    };
    if matches!(&command, Command::Build { .. }) {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(directory)
            .context(
                "document_directory_failed",
                "cannot create document directory",
            )?;
    }
    let directory = fs::canonicalize(directory).context(
        "document_directory_failed",
        "cannot resolve document directory",
    )?;
    if fs::metadata(&directory)
        .context(
            "document_directory_failed",
            "cannot inspect document directory",
        )?
        .permissions()
        .mode()
        & 0o077
        != 0
    {
        return Err(AppError::new(
            "document_directory_invalid",
            "document directory must be private (mode 0700)",
        ));
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(directory.join(".lock"))
        .context("document_lock_failed", "cannot open document lock")?;
    lock.try_lock_exclusive().context(
        "document_run_busy",
        "another process owns this document run",
    )?;
    let run = match command {
        Command::Render { .. } => Run::read(&directory)?,
        Command::Build {
            thread_id, turn_id, ..
        } => {
            let mut run = if directory.join("run.json").exists() {
                Run::read(&directory)?
            } else {
                let snapshot = capture(&thread_id, &turn_id)?;
                let request = build_request(&snapshot)?;
                let run = Run {
                    version: 1,
                    snapshot,
                    request,
                    receipts: Vec::new(),
                    terminal: false,
                };
                run.save(&directory)?;
                run
            };
            let conversation = run.snapshot.conversation();
            if conversation.thread.reference.thread_id != thread_id
                || conversation
                    .turns
                    .last()
                    .map(|turn| turn.reference.turn_id.as_str())
                    != Some(turn_id.as_str())
            {
                return Err(AppError::new(
                    "document_source_conflict",
                    "directory belongs to a different exchange",
                ));
            }
            before_classify(&run.snapshot)?;
            if !run.terminal {
                classify(&mut run, &directory)?;
            }
            run
        }
    };
    // Keep a failed render recoverable even after a terminal runtime result.
    let output = run.render(&directory)?;
    let classification = run.classification().cloned().ok_or_else(|| {
        AppError::new(
            "document_classification_missing",
            "run has no accepted classification",
        )
    })?;
    Ok(DocumentOutput {
        snapshot: run.snapshot,
        classification,
        document: output,
        job_id: run.request.id,
        run_path: directory.join("run.json"),
    })
}

fn capture(thread_id: &str, turn_id: &str) -> AppResult<Snapshot> {
    let mut client = AppServerClient::spawn(ClientConfig {
        stderr_policy: StderrPolicy::Suppress,
        ..ClientConfig::default()
    })
    .context("document_source_unavailable", "cannot start Conversations")?;
    let conversation = client.read_thread(thread_id).context(
        "document_source_unavailable",
        "cannot read complete conversation",
    )?;
    if conversation.thread.reference.thread_id != thread_id {
        return Err(AppError::new(
            "document_source_conflict",
            "Conversations returned a different thread",
        ));
    }
    Snapshot::capture(conversation, turn_id)
        .context("document_source_invalid", "cannot freeze conversation")
}

fn toolset() -> ToolsetRef {
    ToolsetRef {
        provider: "krisis".to_owned(),
        name: "decision-document".to_owned(),
        version: 1,
    }
}

fn build_request(snapshot: &Snapshot) -> AppResult<JobRequestV1> {
    let prompt = snapshot.prompt().context(
        "document_prompt_invalid",
        "cannot prepare full conversation",
    )?;
    let id = format!("krisis-document-{}", uuid::Uuid::now_v7());
    let mut invocation = AgentInvocationV1::new(
        "codex",
        ModelId::new("gpt-5.6-terra"),
        AbsolutePath::new(classifier_cwd(&id)?),
        WorkspaceAccess::None,
        BuiltinToolsV1 {
            local_execution: false,
            web_search: false,
        },
        TimeoutSeconds::new(1_200),
    );
    invocation.reasoning_effort = Some(ReasoningEffort::Medium);
    invocation.toolset = Some(toolset());
    Ok(JobRequestV1::new(
        JobId::new(&id),
        "Classify one exchange for a Krisis document",
        Requester {
            program: "krisis".to_owned(),
            id,
        },
        decisions::document::INSTRUCTIONS,
        prompt,
        invocation,
    ))
}

async fn register(client: &NucleusClient) -> AppResult<()> {
    let input = classification_schema();
    let output = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "oneOf": [
            {"type": "object", "additionalProperties": false, "required": ["accepted"], "properties": {"accepted": {"const": true}}},
            {"type": "object", "additionalProperties": false, "required": ["error"], "properties": {
                "error": {"type": "object", "additionalProperties": false, "required": ["code", "message"], "properties": {
                    "code": {"const": "invalid_structure"}, "message": {"type": "string", "minLength": 1}
                }}
            }}
        ]
    });
    for (id, value) in [(INPUT_SCHEMA, &input), (RESULT_SCHEMA, &output)] {
        let schema = LogSchemaV1::new(
            id,
            id,
            "1",
            "application/schema+json",
            "krisis",
            to_raw_value(value).context("document_contract_invalid", "cannot encode schema")?,
        );
        client.register_schema(&schema).await.context(
            "nucleus_registration_failed",
            "cannot register document schema",
        )?;
    }
    let registration = ToolsetRegistrationV1::new(
        toolset(),
        "nucleus.toolset-definitions.v1",
        ToolsetDefinitionsV1 {
            version: PROTOCOL_VERSION_V1,
            tools: vec![ToolDefinitionV1 {
                name: TOOL_NAME.to_owned(),
                description: "Submit the decision verdict and brief summary for this exchange."
                    .to_owned(),
                input_schema_id: SchemaId::new(INPUT_SCHEMA),
                input_schema: to_raw_value(&input)
                    .context("document_contract_invalid", "cannot encode input schema")?,
            }],
        },
    )
    .context("document_contract_invalid", "cannot build document toolset")?;
    client.register_toolset(&registration).await.context(
        "nucleus_registration_failed",
        "cannot register document toolset",
    )?;
    Ok(())
}

pub(crate) fn doctor() -> AppResult<()> {
    let deployment_run_id = std::env::var("CELL_DEPLOYMENT_RUN_ID").ok();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("nucleus_runtime_failed", "cannot start requester runtime")?;
    runtime.block_on(async {
        let client = NucleusClient::for_current_user()
            .context("nucleus_unavailable", "cannot connect to Nucleus")?;
        require_health(&client, deployment_run_id.as_deref()).await?;
        register(&client).await
    })
}

fn classify(run: &mut Run, directory: &Path) -> AppResult<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("nucleus_runtime_failed", "cannot start requester runtime")?;
    runtime.block_on(async {
        let client = NucleusClient::for_current_user().context("nucleus_unavailable", "cannot connect to Nucleus")?;
        tokio::time::timeout(Duration::from_mins(22), execute(&client, run, directory)).await
            .map_err(|_| AppError::new("document_wait_timeout", "classification wait exceeded 22 minutes; repeat the same command and directory to resume"))?
    })
}

async fn execute(client: &NucleusClient, run: &mut Run, directory: &Path) -> AppResult<()> {
    require_health(client, None).await?;
    register(client).await?;
    let accepted = client.submit_job(&run.request).await.context(
        "nucleus_submit_failed",
        "admission unresolved; resume the same directory",
    )?;
    let digest = run
        .request
        .digest()
        .context("document_request_invalid", "cannot digest request")?;
    if accepted.job_id != run.request.id || accepted.request_digest != digest {
        return Err(AppError::new(
            "nucleus_protocol_error",
            "admission returned a different job or request digest",
        ));
    }
    let mut after = 0;
    loop {
        let calls = client
            .pending_tool_calls(
                &run.request.id,
                &ToolCallsQueryV1 {
                    after,
                    wait_seconds: 1,
                },
            )
            .await
            .context(
                "nucleus_mailbox_failed",
                "mailbox unavailable; resume the same directory",
            )?;
        if calls.job_id != run.request.id {
            return Err(AppError::new(
                "nucleus_protocol_error",
                "mailbox returned a different job",
            ));
        }
        let mut terminal_acknowledgement = false;
        for pending in calls.calls {
            let result = run.accept_call(directory, &pending.call)?;
            if run.classification().is_some() {
                run.render(directory)?;
            }
            match client
                .post_tool_result(&run.request.id, &pending.call.id, &result)
                .await
            {
                Ok(_) => {}
                Err(nucleus_client::ClientError::Api {
                    status: 409, code, ..
                }) if code == "job_terminal" => terminal_acknowledgement = true,
                Err(error) => {
                    return Err(AppError::new(
                        "nucleus_acknowledgement_failed",
                        format!("{error}; result is saved; resume the same directory"),
                    ));
                }
            }
            after = after.max(pending.call.request_sequence);
        }
        let job = client.get_job(&run.request.id).await.context(
            "nucleus_observation_failed",
            "cannot observe terminal state; resume the same directory",
        )?;
        if job.summary.id != run.request.id || job.summary.request_digest != digest {
            return Err(AppError::new(
                "nucleus_protocol_error",
                "observed job or request digest differs",
            ));
        }
        if terminal_acknowledgement && !job.summary.state.is_terminal() {
            return Err(AppError::new(
                "nucleus_protocol_error",
                "Nucleus rejected acknowledgement as terminal but reported a live job",
            ));
        }
        if job.summary.state.is_terminal() {
            run.terminal = true;
            run.save(directory)?;
            if run.classification().is_none() {
                return Err(AppError::new(
                    "document_classification_missing",
                    format!(
                        "job ended {:?} without an accepted classification; no document was produced",
                        job.summary.state
                    ),
                ));
            }
            return Ok(());
        }
    }
}

fn atomic_write(directory: &Path, name: &str, bytes: &[u8]) -> AppResult<()> {
    let temporary = directory.join(format!(".{name}.{}", uuid::Uuid::now_v7()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, directory.join(name))?;
        File::open(directory)?.sync_all()
    })()
    .context(
        "document_write_failed",
        "cannot durably write document state",
    );
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests;
