#![allow(clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::app::{read_utf8, work_label};
use crate::cli::InboxRunArgs;
use crate::config::{Config, InboxConfig};
use crate::corpus::{ReconciliationRecord, now, reconciliation_query, store_work};
use crate::db;
use crate::error::AppError;
use crate::model_runner::{ModelSettings, Runner};
use crate::render::CommandOutput;
use crate::{liaison, resolver};

const QUEUE_VERSION: u32 = 1;
const RECEIPT_VERSION: u32 = 1;

#[derive(Debug)]
struct Spool {
    root: PathBuf,
    incoming: PathBuf,
    processing: PathBuf,
    done: PathBuf,
    failed: PathBuf,
    index: PathBuf,
    lock: PathBuf,
}

impl Spool {
    fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            incoming: root.join("incoming"),
            processing: root.join("processing"),
            done: root.join("done"),
            failed: root.join("failed"),
            index: root.join(".queue.json"),
            lock: root.join(".run.lock"),
        }
    }

    fn create(&self) -> Result<(), AppError> {
        for path in [
            &self.root,
            &self.incoming,
            &self.processing,
            &self.done,
            &self.failed,
        ] {
            fs::create_dir_all(path).map_err(|error| {
                AppError::unexpected(
                    "inbox_directory_failed",
                    format!(
                        "unable to create inbox directory {}: {error}",
                        path.display()
                    ),
                )
            })?;
        }
        Ok(())
    }

    fn acquire_lock(&self) -> Result<File, AppError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&self.lock)
            .map_err(|error| {
                AppError::unexpected(
                    "inbox_lock_failed",
                    format!("unable to open inbox lock {}: {error}", self.lock.display()),
                )
            })?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                AppError::conflict(
                    "inbox_locked",
                    format!("another inbox run holds {}", self.lock.display()),
                )
            } else {
                AppError::unexpected(
                    "inbox_lock_failed",
                    format!("unable to lock {}: {error}", self.lock.display()),
                )
            }
        })?;
        Ok(file)
    }
}

#[derive(Debug, Serialize)]
struct InboxStatus {
    root: String,
    incoming: usize,
    ready: usize,
    settling: usize,
    ignored: usize,
    processing: usize,
    done: usize,
    failed: usize,
    locked: bool,
}

#[derive(Debug, Serialize)]
struct RunSummary {
    root: String,
    max_items: usize,
    max_elapsed_seconds: u64,
    settle_seconds: u64,
    attempted: usize,
    applied: usize,
    recorded: usize,
    failed: usize,
    recovered: usize,
    elapsed_seconds: f64,
    stopped: &'static str,
    remaining: usize,
    items: Vec<JobResult>,
}

#[derive(Debug, Serialize)]
struct JobResult {
    job: String,
    file: String,
    work: Option<String>,
    status: String,
    revision: Option<i64>,
    error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueIndex {
    version: u32,
    next_sequence: u64,
    entries: BTreeMap<String, u64>,
}

impl Default for QueueIndex {
    fn default() -> Self {
        Self {
            version: QUEUE_VERSION,
            next_sequence: 1,
            entries: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
struct IncomingFile {
    path: PathBuf,
    name: OsString,
    key: String,
    sequence: u64,
    identity: FileIdentity,
    ready: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
    size: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
}

#[derive(Debug)]
struct Envelope {
    id: String,
    directory: PathBuf,
    source: PathBuf,
    expected_identity: Option<FileIdentity>,
    receipt: JobReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JobReceipt {
    version: u32,
    id: String,
    original_name: String,
    original_name_base64: String,
    state: String,
    attempts: u32,
    created_at: String,
    started_at: Option<String>,
    completed_at: Option<String>,
    source_sha256: Option<String>,
    work: Option<String>,
    reconciliation_id: Option<i64>,
    model_run_token: Option<String>,
    result_status: Option<String>,
    result_revision: Option<i64>,
    last_error: Option<ReceiptError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptError {
    code: String,
    message: String,
}

#[derive(Debug)]
struct EffectiveRun {
    max_items: usize,
    max_elapsed_seconds: u64,
    settle_seconds: u64,
}

#[derive(Debug)]
enum Completion {
    Applied(i64),
    Recorded,
}

pub(crate) fn run(
    library: &Path,
    config: &Config,
    args: &InboxRunArgs,
    forward_progress: bool,
) -> Result<CommandOutput, AppError> {
    let inbox = config.inbox()?;
    let effective = EffectiveRun::resolve(inbox, args)?;
    let spool = Spool::new(&inbox.root);
    spool.create()?;
    let _lock = spool.acquire_lock()?;
    let started = Instant::now();
    let mut index = read_index(&spool.index)?;
    let (mut incoming, ignored) = scan_incoming(&spool, &mut index, effective.settle_seconds)?;
    write_index(&spool.index, &index)?;
    let mut processing = scan_envelopes(&spool.processing)?;
    processing.sort_by(|left, right| left.id.cmp(&right.id));
    incoming.sort_by(|left, right| {
        left.sequence
            .cmp(&right.sequence)
            .then_with(|| left.name.as_bytes().cmp(right.name.as_bytes()))
    });

    let settings = ModelSettings::new(config.liaison.quality, config.liaison.model.as_deref());
    let runner = Runner::for_program(&config.liaison.codex);
    let mut summary = RunSummary {
        root: spool.root.display().to_string(),
        max_items: effective.max_items,
        max_elapsed_seconds: effective.max_elapsed_seconds,
        settle_seconds: effective.settle_seconds,
        attempted: 0,
        applied: 0,
        recorded: 0,
        failed: 0,
        recovered: 0,
        elapsed_seconds: 0.0,
        stopped: "empty",
        remaining: 0,
        items: Vec::new(),
    };

    for envelope in processing {
        if should_stop(&summary, &effective, started) {
            summary.stopped = stop_reason(&summary, &effective, started);
            break;
        }
        summary.recovered += 1;
        process_one(
            library,
            &spool,
            envelope,
            true,
            &settings,
            &runner,
            forward_progress,
            &mut summary,
        )?;
    }

    for candidate in incoming.iter().filter(|candidate| candidate.ready) {
        if should_stop(&summary, &effective, started) {
            summary.stopped = stop_reason(&summary, &effective, started);
            break;
        }
        let Some(envelope) = claim(&spool, candidate)? else {
            continue;
        };
        index.entries.remove(&candidate.key);
        write_index(&spool.index, &index)?;
        process_one(
            library,
            &spool,
            envelope,
            false,
            &settings,
            &runner,
            forward_progress,
            &mut summary,
        )?;
    }

    if summary.attempted > 0 && summary.stopped == "empty" {
        summary.stopped = "batch_complete";
    }
    let final_status = inspect(&spool, effective.settle_seconds)?;
    summary.remaining = final_status.ready + final_status.processing;
    summary.elapsed_seconds = started.elapsed().as_secs_f64();
    let human = format!(
        "Inbox run: {} attempted, {} applied, {} recorded, {} failed\nRemaining ready or processing: {}\nStopped: {}",
        summary.attempted,
        summary.applied,
        summary.recorded,
        summary.failed,
        summary.remaining,
        summary.stopped,
    );
    let mut value = serde_json::to_value(&summary)?;
    if let Value::Object(object) = &mut value {
        object.insert("ignored".to_owned(), json!(ignored));
    }
    Ok(CommandOutput::new(value, human).mutation())
}

pub(crate) fn status(config: &Config) -> Result<CommandOutput, AppError> {
    let inbox = config.inbox()?;
    let spool = Spool::new(&inbox.root);
    let value = inspect(&spool, inbox.settle_seconds)?;
    let human = format!(
        "Inbox: {}\nIncoming: {} ({} ready, {} settling)\nProcessing: {}\nDone: {}\nFailed: {}\nLocked: {}",
        value.root,
        value.incoming,
        value.ready,
        value.settling,
        value.processing,
        value.done,
        value.failed,
        value.locked,
    );
    Ok(CommandOutput::new(serde_json::to_value(value)?, human))
}

impl EffectiveRun {
    fn resolve(config: &InboxConfig, args: &InboxRunArgs) -> Result<Self, AppError> {
        let resolved = Self {
            max_items: args.max_items.unwrap_or(config.max_items),
            max_elapsed_seconds: args
                .max_elapsed_seconds
                .unwrap_or(config.max_elapsed_seconds),
            settle_seconds: args.settle_seconds.unwrap_or(config.settle_seconds),
        };
        if resolved.max_items == 0 {
            return Err(AppError::invalid(
                "invalid_limit",
                "--max-items must be positive",
            ));
        }
        if resolved.max_elapsed_seconds == 0 {
            return Err(AppError::invalid(
                "invalid_limit",
                "--max-elapsed-seconds must be positive",
            ));
        }
        Ok(resolved)
    }
}

fn should_stop(summary: &RunSummary, effective: &EffectiveRun, started: Instant) -> bool {
    summary.attempted >= effective.max_items
        || summary.attempted > 0
            && started.elapsed() >= Duration::from_secs(effective.max_elapsed_seconds)
}

fn stop_reason(summary: &RunSummary, effective: &EffectiveRun, started: Instant) -> &'static str {
    if summary.attempted >= effective.max_items {
        "max_items"
    } else if started.elapsed() >= Duration::from_secs(effective.max_elapsed_seconds) {
        "max_elapsed"
    } else {
        "batch_complete"
    }
}

#[allow(clippy::too_many_arguments)]
fn process_one(
    library: &Path,
    spool: &Spool,
    mut envelope: Envelope,
    recovered: bool,
    settings: &ModelSettings,
    runner: &Runner,
    forward_progress: bool,
    summary: &mut RunSummary,
) -> Result<(), AppError> {
    if recovered && envelope.receipt.state == "done" {
        let completion = completion_from_receipt(&envelope.receipt)?;
        let (status, revision) = match completion {
            Completion::Applied(revision) => {
                summary.applied += 1;
                ("applied", Some(revision))
            }
            Completion::Recorded => {
                summary.recorded += 1;
                ("recorded", None)
            }
        };
        move_envelope(&envelope, &spool.done)?;
        summary.items.push(JobResult {
            job: envelope.id,
            file: envelope.receipt.original_name,
            work: envelope.receipt.work,
            status: status.to_owned(),
            revision,
            error_code: None,
        });
        return Ok(());
    }
    if recovered && envelope.receipt.state == "failed" {
        let error_code = envelope
            .receipt
            .last_error
            .as_ref()
            .map(|error| error.code.clone());
        move_envelope(&envelope, &spool.failed)?;
        summary.failed += 1;
        summary.items.push(JobResult {
            job: envelope.id,
            file: envelope.receipt.original_name,
            work: envelope.receipt.work,
            status: "failed".to_owned(),
            revision: None,
            error_code,
        });
        return Ok(());
    }
    summary.attempted += 1;
    envelope.receipt.attempts = envelope.receipt.attempts.saturating_add(1);
    "processing".clone_into(&mut envelope.receipt.state);
    envelope.receipt.started_at = Some(now()?);
    envelope.receipt.last_error = None;
    write_receipt(&envelope)?;
    let file = envelope.receipt.original_name.clone();
    match process_work(
        library,
        &mut envelope,
        recovered,
        settings,
        runner,
        forward_progress,
    ) {
        Ok(completion) => {
            let (status, result_revision) = match completion {
                Completion::Applied(revision) => {
                    summary.applied += 1;
                    ("applied", Some(revision))
                }
                Completion::Recorded => {
                    summary.recorded += 1;
                    ("recorded", None)
                }
            };
            "done".clone_into(&mut envelope.receipt.state);
            envelope.receipt.completed_at = Some(now()?);
            envelope.receipt.result_status = Some(status.to_owned());
            envelope.receipt.result_revision = result_revision;
            write_receipt(&envelope)?;
            move_envelope(&envelope, &spool.done)?;
            summary.items.push(JobResult {
                job: envelope.id,
                file,
                work: envelope.receipt.work,
                status: status.to_owned(),
                revision: result_revision,
                error_code: None,
            });
            Ok(())
        }
        Err(error) if permanent_source_error(&error) => {
            let code = error.code().to_owned();
            "failed".clone_into(&mut envelope.receipt.state);
            envelope.receipt.completed_at = Some(now()?);
            envelope.receipt.last_error = Some(ReceiptError {
                code: code.clone(),
                message: error.to_string(),
            });
            write_receipt(&envelope)?;
            move_envelope(&envelope, &spool.failed)?;
            summary.failed += 1;
            summary.items.push(JobResult {
                job: envelope.id,
                file,
                work: envelope.receipt.work,
                status: "failed".to_owned(),
                revision: None,
                error_code: Some(code),
            });
            Ok(())
        }
        Err(error) => {
            envelope.receipt.last_error = Some(ReceiptError {
                code: error.code().to_owned(),
                message: error.to_string(),
            });
            write_receipt(&envelope)?;
            Err(AppError::unexpected(
                "inbox_job_failed",
                format!(
                    "inbox job {} failed and remains retryable: {error}",
                    envelope.id
                ),
            ))
        }
    }
}

fn process_work(
    library: &Path,
    envelope: &mut Envelope,
    recovered: bool,
    settings: &ModelSettings,
    runner: &Runner,
    forward_progress: bool,
) -> Result<Completion, AppError> {
    let before = fs::symlink_metadata(&envelope.source)?;
    if !before.file_type().is_file() {
        return Err(AppError::invalid(
            "invalid_inbox_source",
            format!(
                "inbox source {} is not a regular file",
                envelope.source.display()
            ),
        ));
    }
    let before_identity = FileIdentity::from(&before);
    if envelope
        .expected_identity
        .is_some_and(|expected| expected != before_identity)
    {
        return Err(AppError::unexpected(
            "inbox_source_changed",
            format!(
                "inbox source {} changed before it was read",
                envelope.source.display()
            ),
        ));
    }
    let text = read_utf8(&envelope.source, "work")?;
    let after = fs::symlink_metadata(&envelope.source)?;
    if !after.file_type().is_file() || before_identity != FileIdentity::from(&after) {
        return Err(AppError::unexpected(
            "inbox_source_changed",
            format!(
                "inbox source {} changed while it was read",
                envelope.source.display()
            ),
        ));
    }
    let label = work_label(&envelope.source, None)?;
    let mut connection = db::open_write(library)?;
    let work = store_work(&mut connection, &label, &text)?;
    envelope.receipt.source_sha256 = Some(work.sha256.clone());
    envelope.receipt.work = Some(work.label.clone());
    write_receipt(envelope)?;

    if let Some(record) =
        receipt_reconciliation(&connection, work.id, envelope.receipt.reconciliation_id)?
    {
        match record.status.as_str() {
            "applied" => {
                envelope.receipt.reconciliation_id = Some(record.id);
                return Ok(Completion::Applied(record.applied_revision.ok_or_else(
                    || {
                        AppError::database(
                            "invalid_reconciliation",
                            "an applied reconciliation has no applied revision",
                        )
                    },
                )?));
            }
            "recorded" => {
                envelope.receipt.reconciliation_id = Some(record.id);
                return Ok(Completion::Recorded);
            }
            "pending" => match resolver::apply_record(&mut connection, &record) {
                Ok(applied) => {
                    envelope.receipt.reconciliation_id = Some(record.id);
                    return Ok(Completion::Applied(applied));
                }
                Err(error) if error.code() == "stale_change" => {}
                Err(error) => return Err(error),
            },
            "superseded" => {}
            _ => {
                return Err(AppError::database(
                    "invalid_reconciliation",
                    format!("unknown reconciliation status {:?}", record.status),
                ));
            }
        }
    }
    let run_token = connection.query_row("SELECT lower(hex(randomblob(32)))", [], |row| {
        row.get::<_, String>(0)
    })?;
    drop(connection);
    if recovered && let Some(previous_token) = envelope.receipt.model_run_token.as_deref() {
        liaison::abandon_run(library, previous_token, work.id)?;
    }
    envelope.receipt.model_run_token = Some(run_token.clone());
    write_receipt(envelope)?;

    let record = liaison::integrate_with_runner_token(
        library,
        &work,
        settings,
        forward_progress,
        false,
        runner,
        Some(&run_token),
    )?;
    envelope.receipt.reconciliation_id = Some(record.id);
    write_receipt(envelope)?;
    finish_reconciliation(library, &record)
}

fn receipt_reconciliation(
    connection: &rusqlite::Connection,
    work_id: i64,
    reconciliation_id: Option<i64>,
) -> Result<Option<ReconciliationRecord>, AppError> {
    let Some(reconciliation_id) = reconciliation_id else {
        return Ok(None);
    };
    let record = reconciliation_query(
        connection,
        "SELECT r.id, r.work_id, w.label, r.base_revision, r.status, r.summary, \
                r.submitted_request, r.resolved_reconciliation, r.actor, r.created_at, \
                r.applied_revision \
         FROM reconciliations AS r JOIN works AS w ON w.id = r.work_id \
         WHERE r.id = ?1 AND r.work_id = ?2",
        rusqlite::params![reconciliation_id, work_id],
    )?;
    record.map_or_else(
        || {
            Err(AppError::unexpected(
                "invalid_job_receipt",
                format!("receipt reconciliation {reconciliation_id} does not belong to its work"),
            ))
        },
        |record| Ok(Some(record)),
    )
}

fn finish_reconciliation(
    library: &Path,
    record: &ReconciliationRecord,
) -> Result<Completion, AppError> {
    match record.status.as_str() {
        "recorded" => Ok(Completion::Recorded),
        "pending" => {
            let mut connection = db::open_write(library)?;
            resolver::apply_record(&mut connection, record).map(Completion::Applied)
        }
        "applied" => record
            .applied_revision
            .map(Completion::Applied)
            .ok_or_else(|| {
                AppError::database(
                    "invalid_reconciliation",
                    "an applied reconciliation has no applied revision",
                )
            }),
        _ => Err(AppError::conflict(
            "nothing_to_apply",
            format!("the inbox reconciliation is {}", record.status),
        )),
    }
}

fn completion_from_receipt(receipt: &JobReceipt) -> Result<Completion, AppError> {
    match receipt.result_status.as_deref() {
        Some("applied") => receipt
            .result_revision
            .map(Completion::Applied)
            .ok_or_else(|| {
                AppError::unexpected("invalid_job_receipt", "applied job receipt has no revision")
            }),
        Some("recorded") => Ok(Completion::Recorded),
        _ => Err(AppError::unexpected(
            "invalid_job_receipt",
            "done job receipt has no terminal result",
        )),
    }
}

fn permanent_source_error(error: &AppError) -> bool {
    matches!(
        error.code(),
        "input_not_utf8"
            | "empty_work"
            | "work_name_required"
            | "invalid_label"
            | "invalid_inbox_source"
            | "work_name_exists"
    )
}

fn claim(spool: &Spool, candidate: &IncomingFile) -> Result<Option<Envelope>, AppError> {
    if !path_has_identity(&candidate.path, candidate.identity)? {
        return Ok(None);
    }
    let id = available_job_id(spool, candidate.sequence);
    let directory = spool.processing.join(&id);
    let material = directory.join("material");
    if let Err(error) = create_private_directory(&directory) {
        return Err(error.into());
    }
    if let Err(error) = create_private_directory(&material) {
        let _ = fs::remove_dir(&directory);
        return Err(error.into());
    }
    let destination = material.join(&candidate.name);
    if let Err(error) = fs::rename(&candidate.path, &destination) {
        let _ = fs::remove_dir(&material);
        let _ = fs::remove_dir(&directory);
        return Err(AppError::unexpected(
            "inbox_claim_failed",
            format!(
                "unable to move {} into its job envelope: {error}",
                candidate.path.display()
            ),
        ));
    }
    if !path_has_identity(&destination, candidate.identity)?
        && path_is_missing(&candidate.path)
        && fs::rename(&destination, &candidate.path).is_ok()
    {
        let _ = fs::remove_dir(&material);
        let _ = fs::remove_dir(&directory);
        return Ok(None);
    }
    let receipt = new_receipt(&id, &candidate.name)?;
    let envelope = Envelope {
        id,
        directory,
        source: destination,
        expected_identity: Some(candidate.identity),
        receipt,
    };
    write_receipt(&envelope)?;
    Ok(Some(envelope))
}

fn create_private_directory(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}

fn path_has_identity(path: &Path, expected: FileIdentity) -> Result<bool, AppError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            let actual = FileIdentity::from(&metadata);
            Ok(metadata.file_type().is_file() && actual == expected)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn path_is_missing(path: &Path) -> bool {
    fs::symlink_metadata(path).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
}

fn available_job_id(spool: &Spool, sequence: u64) -> String {
    let base = format!("j{sequence:020}");
    if !job_id_exists(spool, &base) {
        return base;
    }
    for suffix in 1_u64.. {
        let candidate = format!("{base}-{suffix}");
        if !job_id_exists(spool, &candidate) {
            return candidate;
        }
    }
    unreachable!()
}

fn job_id_exists(spool: &Spool, id: &str) -> bool {
    [&spool.processing, &spool.done, &spool.failed]
        .iter()
        .any(|parent| parent.join(id).exists())
}

fn move_envelope(envelope: &Envelope, destination: &Path) -> Result<(), AppError> {
    let target = destination.join(&envelope.id);
    if target.exists() {
        return Err(AppError::conflict(
            "inbox_archive_exists",
            format!("inbox archive already exists: {}", target.display()),
        ));
    }
    fs::rename(&envelope.directory, &target).map_err(|error| {
        AppError::unexpected(
            "inbox_archive_failed",
            format!(
                "unable to move job {} to {}: {error}",
                envelope.id,
                destination.display()
            ),
        )
    })
}

fn write_receipt(envelope: &Envelope) -> Result<(), AppError> {
    write_json_atomic(
        &envelope.directory.join("job.json"),
        &envelope.receipt,
        "job receipt",
    )
}

fn scan_envelopes(path: &Path) -> Result<Vec<Envelope>, AppError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut envelopes = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let directory = entry.path();
        let id = entry.file_name().to_string_lossy().into_owned();
        let receipt_path = directory.join("job.json");
        let material = directory.join("material");
        let source = match sole_material(&material)? {
            Some(source) => source,
            None if !receipt_path.exists() => {
                if material.exists() {
                    fs::remove_dir(&material)?;
                }
                fs::remove_dir(&directory)?;
                continue;
            }
            None => {
                return Err(AppError::unexpected(
                    "invalid_job_envelope",
                    format!(
                        "{} must contain exactly one regular source file",
                        material.display()
                    ),
                ));
            }
        };
        let receipt = if receipt_path.exists() {
            serde_json::from_slice(&fs::read(&receipt_path)?).map_err(|error| {
                AppError::unexpected(
                    "invalid_job_receipt",
                    format!("invalid job receipt {}: {error}", receipt_path.display()),
                )
            })?
        } else {
            let name = source.file_name().ok_or_else(|| {
                AppError::unexpected(
                    "invalid_job_envelope",
                    format!("source {} has no basename", source.display()),
                )
            })?;
            let receipt = new_receipt(&id, name)?;
            let envelope = Envelope {
                id: id.clone(),
                directory: directory.clone(),
                source: source.clone(),
                expected_identity: None,
                receipt: receipt.clone(),
            };
            write_receipt(&envelope)?;
            receipt
        };
        validate_receipt(&receipt, &id, &source)?;
        envelopes.push(Envelope {
            id,
            directory,
            source,
            expected_identity: None,
            receipt,
        });
    }
    Ok(envelopes)
}

fn new_receipt(id: &str, name: &OsStr) -> Result<JobReceipt, AppError> {
    Ok(JobReceipt {
        version: RECEIPT_VERSION,
        id: id.to_owned(),
        original_name: name.to_string_lossy().into_owned(),
        original_name_base64: URL_SAFE_NO_PAD.encode(name.as_bytes()),
        state: "processing".to_owned(),
        attempts: 0,
        created_at: now()?,
        started_at: None,
        completed_at: None,
        source_sha256: None,
        work: None,
        reconciliation_id: None,
        model_run_token: None,
        result_status: None,
        result_revision: None,
        last_error: None,
    })
}

fn validate_receipt(receipt: &JobReceipt, id: &str, source: &Path) -> Result<(), AppError> {
    let encoded_name = source
        .file_name()
        .map(|name| URL_SAFE_NO_PAD.encode(name.as_bytes()));
    if receipt.version != RECEIPT_VERSION
        || receipt.id != id
        || encoded_name.as_deref() != Some(&receipt.original_name_base64)
        || !matches!(receipt.state.as_str(), "processing" | "done" | "failed")
    {
        return Err(AppError::unexpected(
            "invalid_job_receipt",
            format!(
                "job receipt {} does not match its envelope",
                source
                    .parent()
                    .and_then(Path::parent)
                    .unwrap_or(source)
                    .display()
            ),
        ));
    }
    Ok(())
}

fn sole_material(path: &Path) -> Result<Option<PathBuf>, AppError> {
    if !path.exists() {
        return Ok(None);
    }
    let entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    match entries.as_slice() {
        [] => Ok(None),
        [source] => Ok(Some(source.path())),
        _ => Err(AppError::unexpected(
            "invalid_job_envelope",
            format!(
                "{} must contain exactly one regular source file",
                path.display()
            ),
        )),
    }
}

fn scan_incoming(
    spool: &Spool,
    index: &mut QueueIndex,
    settle_seconds: u64,
) -> Result<(Vec<IncomingFile>, usize), AppError> {
    let mut visible = BTreeSet::new();
    let mut files = Vec::new();
    let mut ignored = 0;
    let mut entries = fs::read_dir(&spool.incoming)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|left, right| {
        left.file_name()
            .as_bytes()
            .cmp(right.file_name().as_bytes())
    });
    for entry in entries {
        let name = entry.file_name();
        let kind = entry.file_type()?;
        if hidden_or_temporary(&name) || !kind.is_file() {
            ignored += 1;
            continue;
        }
        let key = URL_SAFE_NO_PAD.encode(name.as_os_str().as_bytes());
        visible.insert(key.clone());
        let sequence = if let Some(sequence) = index.entries.get(&key) {
            *sequence
        } else {
            let sequence = index.next_sequence;
            index.next_sequence = index.next_sequence.checked_add(1).ok_or_else(|| {
                AppError::unexpected("inbox_sequence_overflow", "inbox sequence is exhausted")
            })?;
            index.entries.insert(key.clone(), sequence);
            sequence
        };
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.file_type().is_file() {
            ignored += 1;
            continue;
        }
        files.push(IncomingFile {
            path: entry.path(),
            name,
            key,
            sequence,
            identity: FileIdentity::from(&metadata),
            ready: settled(&metadata, settle_seconds),
        });
    }
    index.entries.retain(|key, _| visible.contains(key));
    Ok((files, ignored))
}

impl From<&fs::Metadata> for FileIdentity {
    fn from(metadata: &fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.len(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        }
    }
}

fn hidden_or_temporary(name: &OsStr) -> bool {
    let bytes = name.as_bytes();
    bytes.starts_with(b".") || bytes.ends_with(b".part")
}

fn settled(metadata: &fs::Metadata, seconds: u64) -> bool {
    if seconds == 0 {
        return true;
    }
    metadata
        .modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= Duration::from_secs(seconds))
}

fn read_index(path: &Path) -> Result<QueueIndex, AppError> {
    if !path.exists() {
        return Ok(QueueIndex::default());
    }
    let index: QueueIndex = serde_json::from_slice(&fs::read(path)?).map_err(|error| {
        AppError::unexpected(
            "invalid_inbox_index",
            format!("invalid inbox index {}: {error}", path.display()),
        )
    })?;
    if index.version != QUEUE_VERSION || index.next_sequence == 0 {
        return Err(AppError::unexpected(
            "invalid_inbox_index",
            format!("unsupported inbox index {}", path.display()),
        ));
    }
    Ok(index)
}

fn write_index(path: &Path, index: &QueueIndex) -> Result<(), AppError> {
    write_json_atomic(path, index, "inbox index")
}

fn write_json_atomic(
    path: &Path,
    value: &impl Serialize,
    description: &str,
) -> Result<(), AppError> {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| AppError::unexpected("invalid_output_path", "output filename is invalid"))?;
    let temporary = path.with_file_name(format!(".{name}.tmp"));
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(&temporary, bytes).map_err(|error| {
        AppError::unexpected(
            "inbox_state_write_failed",
            format!("unable to write {description} {}: {error}", path.display()),
        )
    })?;
    fs::rename(&temporary, path).map_err(|error| {
        AppError::unexpected(
            "inbox_state_write_failed",
            format!(
                "unable to replace {description} {}: {error}",
                path.display()
            ),
        )
    })
}

fn inspect(spool: &Spool, settle_seconds: u64) -> Result<InboxStatus, AppError> {
    let mut incoming = 0;
    let mut ready = 0;
    let mut settling = 0;
    let mut ignored = 0;
    if spool.incoming.exists() {
        for entry in fs::read_dir(&spool.incoming)? {
            let entry = entry?;
            let name = entry.file_name();
            let metadata = fs::symlink_metadata(entry.path())?;
            if hidden_or_temporary(&name) || !metadata.file_type().is_file() {
                ignored += 1;
                continue;
            }
            incoming += 1;
            if settled(&metadata, settle_seconds) {
                ready += 1;
            } else {
                settling += 1;
            }
        }
    }
    Ok(InboxStatus {
        root: spool.root.display().to_string(),
        incoming,
        ready,
        settling,
        ignored,
        processing: count_directories(&spool.processing)?,
        done: count_directories(&spool.done)?,
        failed: count_directories(&spool.failed)?,
        locked: inbox_locked(&spool.lock),
    })
}

fn count_directories(path: &Path) -> Result<usize, AppError> {
    if !path.exists() {
        return Ok(0);
    }
    fs::read_dir(path)?.try_fold(0_usize, |count, entry| {
        let entry = entry?;
        Ok(count + usize::from(entry.file_type()?.is_dir()))
    })
}

fn inbox_locked(path: &Path) -> bool {
    let Ok(file) = OpenOptions::new().read(true).write(true).open(path) else {
        return false;
    };
    if fs2::FileExt::try_lock_exclusive(&file).is_err() {
        return true;
    }
    let _ = fs2::FileExt::unlock(&file);
    false
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::{
        FileIdentity, QueueIndex, Spool, path_has_identity, read_index, scan_incoming, write_index,
    };

    #[test]
    fn queue_index_preserves_first_seen_order() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let spool = Spool::new(directory.path());
        spool.create()?;
        fs::write(spool.incoming.join("b.txt"), "b")?;
        let mut index = QueueIndex::default();
        let (first, _) = scan_incoming(&spool, &mut index, 0)?;
        write_index(&spool.index, &index)?;
        fs::write(spool.incoming.join("a.txt"), "a")?;
        let mut index = read_index(&spool.index)?;
        let (mut second, _) = scan_incoming(&spool, &mut index, 0)?;
        second.sort_by_key(|entry| entry.sequence);
        assert_eq!(first[0].sequence, second[0].sequence);
        assert_eq!(second[0].name, "b.txt");
        assert_eq!(second[1].name, "a.txt");
        Ok(())
    }

    #[test]
    fn claimed_identity_rejects_replacements_and_symlinks() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("source.txt");
        let original = directory.path().join("original.txt");
        fs::write(&source, "same bytes")?;
        let identity = FileIdentity::from(&fs::symlink_metadata(&source)?);
        assert!(path_has_identity(&source, identity)?);

        fs::rename(&source, &original)?;
        fs::write(&source, "same bytes")?;
        assert!(!path_has_identity(&source, identity)?);

        fs::remove_file(&source)?;
        symlink(&original, &source)?;
        assert!(!path_has_identity(&source, identity)?);
        Ok(())
    }
}
