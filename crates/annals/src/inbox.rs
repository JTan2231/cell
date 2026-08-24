#![allow(clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

use crate::app::{read_utf8, work_label};
use crate::cli::InboxRunArgs;
use crate::config::Config;
use crate::corpus::{
    ReconciliationRecord, now, reconciliation_by_id, store_ingested_work_with_optional_label,
};
use crate::db;
use crate::error::AppError;
use crate::model_runner::{ModelSettings, Runner};
use crate::render::CommandOutput;
use crate::{ingestion, liaison, resolver};

const QUEUE_VERSION: u32 = 4;
const RECEIPT_VERSION: u32 = 3;

#[derive(Debug)]
struct Spool {
    root: PathBuf,
    incoming: PathBuf,
    queued: PathBuf,
    processing: PathBuf,
    done: PathBuf,
    duplicates: PathBuf,
    failed: PathBuf,
    index: PathBuf,
    lock: PathBuf,
    control_lock: PathBuf,
    paused: PathBuf,
    maintenance: PathBuf,
}

impl Spool {
    fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            incoming: root.join("incoming"),
            queued: root.join("queued"),
            processing: root.join("processing"),
            done: root.join("done"),
            duplicates: root.join("duplicates"),
            failed: root.join("failed"),
            index: root.join(".queue.json"),
            lock: root.join(".run.lock"),
            control_lock: root.join(".control.lock"),
            paused: root.join(".paused"),
            maintenance: root.join(".maintenance"),
        }
    }

    fn create(&self) -> Result<(), AppError> {
        for path in [
            &self.root,
            &self.incoming,
            &self.queued,
            &self.processing,
            &self.done,
            &self.duplicates,
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

    fn acquire_control_lock(&self) -> Result<File, AppError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&self.control_lock)
            .map_err(|error| {
                AppError::unexpected(
                    "inbox_control_lock_failed",
                    format!(
                        "unable to open inbox control lock {}: {error}",
                        self.control_lock.display()
                    ),
                )
            })?;
        fs2::FileExt::lock_exclusive(&file).map_err(|error| {
            AppError::unexpected(
                "inbox_control_lock_failed",
                format!(
                    "unable to lock inbox control state {}: {error}",
                    self.control_lock.display()
                ),
            )
        })?;
        Ok(file)
    }

    fn pause_requested(&self) -> Result<bool, AppError> {
        marker_requested(
            &self.paused,
            "inbox_pause_invalid",
            "inbox_pause_failed",
            "pause",
        )
    }

    fn maintenance_requested(&self) -> Result<bool, AppError> {
        marker_requested(
            &self.maintenance,
            "inbox_maintenance_invalid",
            "inbox_maintenance_failed",
            "maintenance",
        )
    }
}

fn marker_requested(
    path: &Path,
    invalid_code: &'static str,
    failed_code: &'static str,
    description: &str,
) -> Result<bool, AppError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err(AppError::unexpected(
            invalid_code,
            format!(
                "inbox {description} marker is not a regular file: {}",
                path.display()
            ),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(AppError::unexpected(
            failed_code,
            format!(
                "unable to inspect inbox {description} marker {}: {error}",
                path.display()
            ),
        )),
    }
}

fn create_pause_marker(spool: &Spool) -> Result<bool, AppError> {
    if spool.pause_requested()? {
        return Ok(false);
    }
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&spool.paused)
    {
        Ok(file) => {
            drop(file);
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            spool.pause_requested().map(|_| false)
        }
        Err(error) => Err(AppError::unexpected(
            "inbox_pause_failed",
            format!(
                "unable to create inbox pause marker {}: {error}",
                spool.paused.display()
            ),
        )),
    }
}

fn remove_pause_marker(spool: &Spool) -> Result<bool, AppError> {
    if !spool.pause_requested()? {
        return Ok(false);
    }
    match fs::remove_file(&spool.paused) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(AppError::unexpected(
            "inbox_pause_failed",
            format!(
                "unable to remove inbox pause marker {}: {error}",
                spool.paused.display()
            ),
        )),
    }
}

#[derive(Debug, Serialize)]
struct InboxStatus {
    root: String,
    incoming: usize,
    ready: usize,
    settling: usize,
    ignored: usize,
    queued: usize,
    next_job: Option<RegisteredJob>,
    processing: usize,
    done: usize,
    duplicates: usize,
    failed: usize,
    locked: bool,
    paused: bool,
    maintenance: bool,
}

#[derive(Debug, Serialize)]
struct RunSummary {
    root: String,
    settle_seconds: u64,
    registered: usize,
    attempted: usize,
    applied: usize,
    recorded: usize,
    duplicates: usize,
    failed: usize,
    recovered: usize,
    remaining: usize,
    settling: usize,
    ignored: usize,
    elapsed_seconds: f64,
    queue_drained: bool,
    stopped_for_pause: bool,
    stopped_for_maintenance: bool,
}

#[derive(Debug, Serialize)]
struct RegistrationSummary {
    root: String,
    settle_seconds: u64,
    registered: usize,
    jobs: Vec<RegisteredJob>,
    queued: usize,
    ready: usize,
    settling: usize,
    ignored: usize,
    paused: bool,
    maintenance: bool,
}

#[derive(Debug, Serialize)]
struct RegisteredJob {
    id: String,
    sequence: u64,
    source_name: String,
    registered_at: String,
}

#[derive(Debug, Serialize)]
struct BacklogImportSummary {
    source: String,
    destination: String,
    imported: usize,
    queued: usize,
}

struct BacklogSource {
    path: PathBuf,
    name: OsString,
    metadata: ingestion::SourceMetadata,
}

impl From<&Envelope> for RegisteredJob {
    fn from(envelope: &Envelope) -> Self {
        Self {
            id: envelope.id.clone(),
            sequence: envelope.receipt.sequence,
            source_name: envelope.receipt.original_name.clone(),
            registered_at: envelope.receipt.claimed_at.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)]
struct PauseSummary {
    root: String,
    paused: bool,
    changed: bool,
    locked: bool,
    maintenance: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueIndex {
    version: u32,
    next_sequence: u64,
    entries: BTreeMap<String, QueueEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyQueueIndex {
    version: u32,
    next_sequence: u64,
    entries: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueEntry {
    sequence: u64,
    first_seen_at: String,
    identity: FileIdentity,
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
    metadata: ingestion::SourceMetadata,
    ready: bool,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
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
    sequence: u64,
    original_name: String,
    original_name_base64: String,
    state: String,
    attempts: u32,
    delivery_key: String,
    ingestion_id: Option<i64>,
    source_size_bytes: Option<u64>,
    source_created_at: Option<String>,
    source_modified_at: Option<String>,
    first_seen_at: String,
    claimed_at: String,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionTwoJobReceipt {
    version: u32,
    id: String,
    original_name: String,
    original_name_base64: String,
    state: String,
    attempts: u32,
    delivery_key: String,
    ingestion_id: Option<i64>,
    source_size_bytes: Option<u64>,
    source_created_at: Option<String>,
    source_modified_at: Option<String>,
    first_seen_at: String,
    claimed_at: String,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyJobReceipt {
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

#[derive(Debug, Deserialize)]
struct StoredFormatVersion {
    version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptError {
    code: String,
    message: String,
}

#[derive(Debug)]
enum Completion {
    Applied(i64),
    Recorded,
    Duplicate,
}

pub(crate) fn run(
    library: &Path,
    config: &Config,
    args: &InboxRunArgs,
    forward_progress: bool,
) -> Result<CommandOutput, AppError> {
    let inbox = config.inbox()?;
    let settle_seconds = args.settle_seconds.unwrap_or(inbox.settle_seconds);
    let spool = Spool::new(&inbox.root);
    spool.create()?;
    let _lock = spool.acquire_lock()?;
    let started = Instant::now();
    let recovered_envelopes = if spool.maintenance_requested()? {
        let index = read_index(&spool)?;
        recover_envelopes(library, &spool, &index, false)?
    } else {
        let _control = spool.acquire_control_lock()?;
        let persist_repairs = !spool.maintenance_requested()?;
        let index = read_index(&spool)?;
        recover_envelopes(library, &spool, &index, persist_repairs)?
    };
    let recovered = recovered_envelopes.len();
    let mut queue = BTreeMap::new();
    for envelope in recovered_envelopes {
        insert_queued(&mut queue, envelope)?;
    }

    let settings = ModelSettings::new(config.liaison.quality, config.liaison.model.as_deref());
    let runner = Runner::for_program(&config.liaison.codex);
    let mut summary = RunSummary {
        root: spool.root.display().to_string(),
        settle_seconds,
        registered: 0,
        attempted: 0,
        applied: 0,
        recorded: 0,
        duplicates: 0,
        failed: 0,
        recovered,
        remaining: 0,
        settling: 0,
        ignored: 0,
        elapsed_seconds: 0.0,
        queue_drained: false,
        stopped_for_pause: false,
        stopped_for_maintenance: false,
    };

    loop {
        if spool.maintenance_requested()? {
            summary.stopped_for_maintenance = true;
            break;
        }
        let control = spool.acquire_control_lock()?;
        if spool.maintenance_requested()? {
            summary.stopped_for_maintenance = true;
            break;
        }
        let registered = register_settled_locked(&spool, settle_seconds)?;
        summary.registered = summary.registered.saturating_add(registered.len());
        for envelope in registered {
            insert_queued(&mut queue, envelope)?;
        }
        refresh_queued(&spool, &mut queue)?;
        if spool.maintenance_requested()? {
            summary.stopped_for_maintenance = true;
            break;
        }
        if spool.pause_requested()? {
            summary.stopped_for_pause = true;
            break;
        }
        let Some((key, envelope)) = queue.pop_first() else {
            break;
        };
        let envelope = if envelope.receipt.state == "queued" {
            dispatch(&spool, envelope)?
        } else {
            envelope
        };
        drop(control);
        process_one(
            library,
            &spool,
            envelope,
            &settings,
            &runner,
            forward_progress,
            &mut summary,
        )?;
        debug_assert!(!queue.contains_key(&key));
    }

    let _control = spool.acquire_control_lock()?;
    let final_status = inspect(&spool, settle_seconds)?;
    summary.remaining = final_status.ready + final_status.queued + final_status.processing;
    summary.settling = final_status.settling;
    summary.ignored = final_status.ignored;
    summary.elapsed_seconds = started.elapsed().as_secs_f64();
    summary.queue_drained = summary.remaining == 0;
    let human = format!(
        "Inbox run: {} registered, {} attempted, {} applied, {} recorded, {} duplicates, {} failed\nRemaining work: {} ready, queued, or processing; settling: {}\nQueue drained: {}; stopped for pause: {}; stopped for maintenance: {}",
        summary.registered,
        summary.attempted,
        summary.applied,
        summary.recorded,
        summary.duplicates,
        summary.failed,
        summary.remaining,
        summary.settling,
        summary.queue_drained,
        summary.stopped_for_pause,
        summary.stopped_for_maintenance,
    );
    Ok(CommandOutput::new(serde_json::to_value(summary)?, human).mutation())
}

pub(crate) fn register(config: &Config, args: &InboxRunArgs) -> Result<CommandOutput, AppError> {
    let inbox = config.inbox()?;
    let settle_seconds = args.settle_seconds.unwrap_or(inbox.settle_seconds);
    let spool = Spool::new(&inbox.root);
    spool.create()?;
    let registered = register_settled_guarded(&spool, settle_seconds)?;
    let _control = spool.acquire_control_lock()?;
    let status = inspect(&spool, settle_seconds)?;
    let jobs = registered
        .iter()
        .map(RegisteredJob::from)
        .collect::<Vec<_>>();
    let summary = RegistrationSummary {
        root: spool.root.display().to_string(),
        settle_seconds,
        registered: registered.len(),
        jobs,
        queued: status.queued,
        ready: status.ready,
        settling: status.settling,
        ignored: status.ignored,
        paused: status.paused,
        maintenance: status.maintenance,
    };
    let human = format!(
        "Registered {} inbox {}; {} queued, {} ready, {} settling{}",
        summary.registered,
        if summary.registered == 1 {
            "job"
        } else {
            "jobs"
        },
        summary.queued,
        summary.ready,
        summary.settling,
        if summary.maintenance {
            "; inbox is stopped for maintenance"
        } else if summary.paused {
            "; inbox is paused"
        } else {
            ""
        },
    );
    Ok(CommandOutput::new(serde_json::to_value(summary)?, human).mutation())
}

/// Copy the uncompleted FIFO from an archived spool into a fresh spool.
///
/// This is deliberately stricter than ordinary registration.  Deployment
/// must hold both dispatch barriers, and the destination cannot already own
/// queue history.  Old receipts refer to the retired library, so every source
/// becomes a new unstarted envelope with a new delivery key.
pub(crate) fn import_backlog(
    config: &Config,
    source_root: &Path,
) -> Result<CommandOutput, AppError> {
    let inbox = config.inbox()?;
    let destination = Spool::new(&inbox.root);
    destination.create()?;
    let _run = destination.acquire_lock()?;
    let _control = destination.acquire_control_lock()?;
    if !destination.pause_requested()? || !destination.maintenance_requested()? {
        return Err(AppError::conflict(
            "backlog_import_not_quiesced",
            "backlog import requires both inbox pause and maintenance",
        ));
    }

    let source = Spool::new(source_root);
    let source_canonical = fs::canonicalize(&source.root).map_err(|error| {
        AppError::not_found(
            "backlog_source_not_found",
            format!(
                "unable to open archived inbox {}: {error}",
                source.root.display()
            ),
        )
    })?;
    let destination_canonical = fs::canonicalize(&destination.root)?;
    if source_canonical == destination_canonical {
        return Err(AppError::invalid(
            "backlog_source_is_destination",
            "the archived and destination inboxes must differ",
        ));
    }

    let status = inspect(&destination, inbox.settle_seconds)?;
    if status.queued != 0
        || status.processing != 0
        || status.done != 0
        || status.duplicates != 0
        || status.failed != 0
    {
        return Err(AppError::conflict(
            "backlog_destination_not_fresh",
            "backlog import requires a destination with no envelope history",
        ));
    }
    let mut destination_index = read_index(&destination)?;
    if destination_index.next_sequence != 1 || !destination_index.entries.is_empty() {
        return Err(AppError::conflict(
            "backlog_destination_not_fresh",
            "backlog import requires a fresh queue index",
        ));
    }

    let backlog = backlog_sources(&source)?;
    for item in &backlog {
        let sequence = destination_index.next_sequence;
        destination_index.next_sequence = sequence.checked_add(1).ok_or_else(|| {
            AppError::unexpected("inbox_sequence_overflow", "inbox sequence is exhausted")
        })?;
        import_backlog_source(&destination, item, sequence)?;
        write_index(&destination.index, &destination_index)?;
    }

    let final_status = inspect(&destination, inbox.settle_seconds)?;
    let summary = BacklogImportSummary {
        source: source.root.display().to_string(),
        destination: destination.root.display().to_string(),
        imported: backlog.len(),
        queued: final_status.queued,
    };
    let human = format!(
        "Imported {} backlog {} into the fresh inbox; {} queued",
        summary.imported,
        if summary.imported == 1 { "job" } else { "jobs" },
        summary.queued,
    );
    Ok(CommandOutput::new(serde_json::to_value(summary)?, human).mutation())
}

fn backlog_sources(spool: &Spool) -> Result<Vec<BacklogSource>, AppError> {
    let mut source_index = read_index(spool)?;
    repair_sequence_high_water(spool, &mut source_index)?;
    let mut ordered = BTreeMap::<(u64, String), BacklogSource>::new();
    for directory in [&spool.processing, &spool.queued] {
        for scanned in scan_envelopes_at(directory, &source_index, false)? {
            let receipt = &scanned.envelope.receipt;
            let name = scanned
                .envelope
                .source
                .file_name()
                .ok_or_else(|| {
                    AppError::unexpected(
                        "invalid_job_envelope",
                        format!(
                            "source {} has no basename",
                            scanned.envelope.source.display()
                        ),
                    )
                })?
                .to_os_string();
            let metadata =
                metadata_for_recovered_source(&scanned.envelope.source, &receipt.first_seen_at)?;
            let key = (receipt.sequence, scanned.envelope.id.clone());
            if ordered
                .insert(
                    key,
                    BacklogSource {
                        path: scanned.envelope.source,
                        name,
                        metadata,
                    },
                )
                .is_some()
            {
                return Err(AppError::unexpected(
                    "invalid_job_envelope",
                    "archived inbox contains a duplicate job identity",
                ));
            }
        }
    }

    let (incoming, _) = scan_incoming(spool, &mut source_index, 0)?;
    for candidate in incoming {
        ordered.insert(
            (candidate.sequence, candidate.key),
            BacklogSource {
                path: candidate.path,
                name: candidate.name,
                metadata: candidate.metadata,
            },
        );
    }
    Ok(ordered.into_values().collect())
}

fn import_backlog_source(
    spool: &Spool,
    source: &BacklogSource,
    sequence: u64,
) -> Result<(), AppError> {
    let source_metadata = fs::symlink_metadata(&source.path)?;
    if !source_metadata.file_type().is_file() {
        return Err(AppError::unexpected(
            "invalid_job_envelope",
            format!(
                "backlog source is not a regular file: {}",
                source.path.display()
            ),
        ));
    }
    let id = available_job_id(spool, sequence);
    let directory = spool.queued.join(&id);
    let material = directory.join("material");
    create_private_directory(&directory)?;
    if let Err(error) = create_private_directory(&material) {
        let _ = fs::remove_dir(&directory);
        return Err(error.into());
    }
    let destination = material.join(&source.name);
    if let Err(error) = fs::copy(&source.path, &destination) {
        let _ = fs::remove_dir(&material);
        let _ = fs::remove_dir(&directory);
        return Err(AppError::unexpected(
            "backlog_import_failed",
            format!(
                "unable to copy {} into the fresh inbox: {error}",
                source.path.display()
            ),
        ));
    }
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o600))?;
    let receipt = new_receipt(&id, sequence, &source.name, &source.metadata)?;
    let envelope = Envelope {
        id,
        directory,
        expected_identity: Some(FileIdentity::from(&fs::symlink_metadata(&destination)?)),
        source: destination,
        receipt,
    };
    write_receipt(&envelope)
}

pub(crate) fn pause(config: &Config) -> Result<CommandOutput, AppError> {
    set_pause(config, true)
}

pub(crate) fn resume(config: &Config) -> Result<CommandOutput, AppError> {
    set_pause(config, false)
}

fn set_pause(config: &Config, paused: bool) -> Result<CommandOutput, AppError> {
    let inbox = config.inbox()?;
    let spool = Spool::new(&inbox.root);
    spool.create()?;
    let _control = spool.acquire_control_lock()?;
    let changed = if paused {
        create_pause_marker(&spool)?
    } else {
        remove_pause_marker(&spool)?
    };
    let summary = PauseSummary {
        root: spool.root.display().to_string(),
        paused: spool.pause_requested()?,
        changed,
        locked: inbox_locked(&spool.lock),
        maintenance: spool.maintenance_requested()?,
    };
    let human = if paused {
        if changed {
            format!(
                "Inbox pause requested at {}; any active source delivery may finish",
                summary.root
            )
        } else {
            format!("Inbox at {} is already paused", summary.root)
        }
    } else if changed {
        if summary.maintenance {
            format!(
                "Inbox pause cleared at {}; maintenance still prevents processing",
                summary.root
            )
        } else {
            format!(
                "Inbox resumed at {}; queued jobs are eligible for the next inbox run",
                summary.root
            )
        }
    } else {
        format!("Inbox at {} was not paused", summary.root)
    };
    Ok(CommandOutput::new(serde_json::to_value(summary)?, human).mutation())
}

pub(crate) fn status(config: &Config) -> Result<CommandOutput, AppError> {
    let inbox = config.inbox()?;
    let spool = Spool::new(&inbox.root);
    let _control = spool
        .root
        .exists()
        .then(|| spool.acquire_control_lock())
        .transpose()?;
    let value = inspect(&spool, inbox.settle_seconds)?;
    let next = value.next_job.as_ref().map_or_else(
        || "none".to_owned(),
        |job| format!("{} ({})", job.id, job.source_name),
    );
    let human = format!(
        "Inbox: {}\nIncoming: {} ({} ready, {} settling)\nQueued: {}\nNext queued: {}\nProcessing: {}\nDone: {}\nDuplicates: {}\nFailed: {}\nLocked: {}\nPaused: {}\nMaintenance: {}",
        value.root,
        value.incoming,
        value.ready,
        value.settling,
        value.queued,
        next,
        value.processing,
        value.done,
        value.duplicates,
        value.failed,
        value.locked,
        value.paused,
        value.maintenance,
    );
    Ok(CommandOutput::new(serde_json::to_value(value)?, human))
}

#[allow(clippy::too_many_arguments)]
fn process_one(
    library: &Path,
    spool: &Spool,
    mut envelope: Envelope,
    settings: &ModelSettings,
    runner: &Runner,
    forward_progress: bool,
    summary: &mut RunSummary,
) -> Result<(), AppError> {
    let ingestion_id = ensure_ingestion(library, &mut envelope)?;
    if envelope.receipt.state == "done" {
        let completion = completion_from_receipt(&envelope.receipt)?;
        let destination = match completion {
            Completion::Applied(_) => {
                summary.applied += 1;
                &spool.done
            }
            Completion::Recorded => {
                summary.recorded += 1;
                &spool.done
            }
            Completion::Duplicate => {
                summary.duplicates += 1;
                &spool.duplicates
            }
        };
        move_envelope(&envelope, destination)?;
        return Ok(());
    }
    if envelope.receipt.state == "failed" {
        move_envelope(&envelope, &spool.failed)?;
        summary.failed += 1;
        return Ok(());
    }
    summary.attempted += 1;
    envelope.receipt.attempts = envelope.receipt.attempts.saturating_add(1);
    "processing".clone_into(&mut envelope.receipt.state);
    envelope.receipt.started_at = Some(now()?);
    envelope.receipt.last_error = None;
    write_receipt(&envelope)?;
    match process_work(
        library,
        &mut envelope,
        ingestion_id,
        settings,
        runner,
        forward_progress,
    ) {
        Ok(completion) => {
            let (status, result_revision, destination) = match completion {
                Completion::Applied(revision) => {
                    summary.applied += 1;
                    ("applied", Some(revision), &spool.done)
                }
                Completion::Recorded => {
                    summary.recorded += 1;
                    ("recorded", None, &spool.done)
                }
                Completion::Duplicate => {
                    summary.duplicates += 1;
                    ("retained", None, &spool.duplicates)
                }
            };
            let connection = db::open_write(library)?;
            ingestion::complete(&connection, ingestion_id, status, result_revision)?;
            "done".clone_into(&mut envelope.receipt.state);
            envelope.receipt.completed_at = Some(now()?);
            envelope.receipt.result_status = Some(status.to_owned());
            envelope.receipt.result_revision = result_revision;
            write_receipt(&envelope)?;
            move_envelope(&envelope, destination)?;
            Ok(())
        }
        Err(error) if permanent_source_error(&error) => {
            let code = error.code().to_owned();
            let connection = db::open_write(library)?;
            ingestion::fail(&connection, ingestion_id, &error)?;
            "failed".clone_into(&mut envelope.receipt.state);
            envelope.receipt.completed_at = Some(now()?);
            envelope.receipt.last_error = Some(ReceiptError {
                code: code.clone(),
                message: error.to_string(),
            });
            write_receipt(&envelope)?;
            move_envelope(&envelope, &spool.failed)?;
            summary.failed += 1;
            Ok(())
        }
        Err(error) => {
            let connection = db::open_write(library)?;
            ingestion::record_retryable_error(&connection, ingestion_id, &error)?;
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
    ingestion_id: i64,
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
    let label = work_label(&envelope.source, None).ok();
    let mut connection = db::open_write(library)?;
    let stored = store_ingested_work_with_optional_label(
        &mut connection,
        ingestion_id,
        label.as_deref(),
        &text,
    )?;
    let work = stored.work;
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
    if !stored.new_work {
        drop(connection);
        if let Some(previous_token) = envelope.receipt.model_run_token.as_deref() {
            liaison::abandon_run(library, previous_token, work.id)?;
        }
        return Ok(Completion::Duplicate);
    }
    let run_token = connection.query_row("SELECT lower(hex(randomblob(32)))", [], |row| {
        row.get::<_, String>(0)
    })?;
    drop(connection);
    if let Some(previous_token) = envelope.receipt.model_run_token.as_deref() {
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
    let record = reconciliation_by_id(connection, reconciliation_id)?;
    if record.work_id != work_id {
        return Err(AppError::unexpected(
            "invalid_job_receipt",
            format!("receipt reconciliation {reconciliation_id} does not belong to its work"),
        ));
    }
    Ok(Some(record))
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
        Some("retained") if receipt.result_revision.is_none() => Ok(Completion::Duplicate),
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

fn register_settled_guarded(spool: &Spool, settle_seconds: u64) -> Result<Vec<Envelope>, AppError> {
    let _control = spool.acquire_control_lock()?;
    if spool.maintenance_requested()? {
        return Ok(Vec::new());
    }
    register_settled_locked(spool, settle_seconds)
}

fn register_settled_locked(spool: &Spool, settle_seconds: u64) -> Result<Vec<Envelope>, AppError> {
    let mut index = read_index(spool)?;
    repair_sequence_high_water(spool, &mut index)?;
    let (mut incoming, _) = scan_incoming(spool, &mut index, settle_seconds)?;
    write_index(&spool.index, &index)?;
    incoming.sort_by(|left, right| {
        left.sequence
            .cmp(&right.sequence)
            .then_with(|| left.name.as_bytes().cmp(right.name.as_bytes()))
    });

    let mut registered = Vec::new();
    for candidate in incoming.iter().filter(|candidate| candidate.ready) {
        let Some(envelope) = register_candidate(spool, candidate)? else {
            continue;
        };
        index.entries.remove(&candidate.key);
        write_index(&spool.index, &index)?;
        registered.push(envelope);
    }
    Ok(registered)
}

fn insert_queued(
    queue: &mut BTreeMap<(u64, String), Envelope>,
    envelope: Envelope,
) -> Result<(), AppError> {
    let id = envelope.id.clone();
    let key = (envelope.receipt.sequence, envelope.id.clone());
    match queue.entry(key) {
        Entry::Vacant(entry) => {
            entry.insert(envelope);
            Ok(())
        }
        Entry::Occupied(_) => Err(AppError::unexpected(
            "invalid_job_envelope",
            format!("duplicate inbox job identifier {id}"),
        )),
    }
}

fn register_candidate(
    spool: &Spool,
    candidate: &IncomingFile,
) -> Result<Option<Envelope>, AppError> {
    if !path_has_identity(&candidate.path, candidate.identity)? {
        return Ok(None);
    }
    let id = available_job_id(spool, candidate.sequence);
    let directory = spool.queued.join(&id);
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
    let receipt = new_receipt(
        &id,
        candidate.sequence,
        &candidate.name,
        &candidate.metadata,
    )?;
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
    [
        &spool.queued,
        &spool.processing,
        &spool.done,
        &spool.duplicates,
        &spool.failed,
    ]
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

fn dispatch(spool: &Spool, mut envelope: Envelope) -> Result<Envelope, AppError> {
    let target = spool.processing.join(&envelope.id);
    if target.exists() {
        return Err(AppError::conflict(
            "inbox_processing_exists",
            format!(
                "inbox processing envelope already exists: {}",
                target.display()
            ),
        ));
    }
    fs::rename(&envelope.directory, &target).map_err(|error| {
        AppError::unexpected(
            "inbox_dispatch_failed",
            format!(
                "unable to dispatch inbox job {} to {}: {error}",
                envelope.id,
                target.display()
            ),
        )
    })?;
    let source_name = envelope.source.file_name().ok_or_else(|| {
        AppError::unexpected(
            "invalid_job_envelope",
            format!("inbox job {} has no source filename", envelope.id),
        )
    })?;
    envelope.directory = target;
    envelope.source = envelope.directory.join("material").join(source_name);
    "processing".clone_into(&mut envelope.receipt.state);
    write_receipt(&envelope)?;
    Ok(envelope)
}

fn write_receipt(envelope: &Envelope) -> Result<(), AppError> {
    write_json_atomic(
        &envelope.directory.join("job.json"),
        &envelope.receipt,
        "job receipt",
    )
}

fn recover_envelopes(
    library: &Path,
    spool: &Spool,
    index: &QueueIndex,
    persist_repairs: bool,
) -> Result<Vec<Envelope>, AppError> {
    let queued = scan_envelopes_at(&spool.queued, index, persist_repairs)?;
    let processing = scan_envelopes_at(&spool.processing, index, persist_repairs)?;
    if !persist_repairs {
        return Ok(queued
            .into_iter()
            .chain(processing)
            .map(|scanned| scanned.envelope)
            .collect());
    }

    let connection = db::open_read(library)?;
    let mut recovered = Vec::with_capacity(queued.len() + processing.len());
    for mut scanned in queued {
        let legacy_queued = scanned.stored_version < RECEIPT_VERSION
            && receipt_has_no_processing_progress(&scanned.envelope.receipt)
            && !delivery_record_exists(&connection, &scanned.envelope.receipt.delivery_key)?;
        if scanned.envelope.receipt.state == "queued" || legacy_queued {
            "queued".clone_into(&mut scanned.envelope.receipt.state);
            if scanned.stored_version != RECEIPT_VERSION || legacy_queued {
                write_receipt(&scanned.envelope)?;
            }
            recovered.push(scanned.envelope);
            continue;
        }
        return Err(AppError::unexpected(
            "invalid_job_envelope",
            format!(
                "queued inbox job {} has state {}",
                scanned.envelope.id, scanned.envelope.receipt.state
            ),
        ));
    }

    for mut scanned in processing {
        let legacy_unstarted = scanned.stored_version < RECEIPT_VERSION
            && receipt_has_no_processing_progress(&scanned.envelope.receipt)
            && !delivery_record_exists(&connection, &scanned.envelope.receipt.delivery_key)?;
        if scanned.stored_version == 0 || legacy_unstarted {
            let mut envelope = relocate_envelope(scanned.envelope, &spool.queued)?;
            "queued".clone_into(&mut envelope.receipt.state);
            write_receipt(&envelope)?;
            recovered.push(envelope);
            continue;
        }
        if scanned.envelope.receipt.state == "queued" {
            "processing".clone_into(&mut scanned.envelope.receipt.state);
            write_receipt(&scanned.envelope)?;
        } else if scanned.stored_version != RECEIPT_VERSION {
            write_receipt(&scanned.envelope)?;
        }
        recovered.push(scanned.envelope);
    }
    validate_recovered_order(&recovered)?;
    Ok(recovered)
}

fn receipt_has_no_processing_progress(receipt: &JobReceipt) -> bool {
    receipt.attempts == 0
        && receipt.ingestion_id.is_none()
        && receipt.started_at.is_none()
        && receipt.completed_at.is_none()
        && receipt.source_sha256.is_none()
        && receipt.work.is_none()
        && receipt.reconciliation_id.is_none()
        && receipt.model_run_token.is_none()
        && receipt.result_status.is_none()
        && receipt.result_revision.is_none()
        && receipt.last_error.is_none()
}

fn delivery_record_exists(
    connection: &rusqlite::Connection,
    delivery_key: &str,
) -> Result<bool, AppError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM ingestions WHERE delivery_key = ?1)",
            [delivery_key],
            |row| row.get(0),
        )
        .map_err(AppError::from)
}

fn relocate_envelope(mut envelope: Envelope, destination: &Path) -> Result<Envelope, AppError> {
    let target = destination.join(&envelope.id);
    if target.exists() {
        return Err(AppError::conflict(
            "inbox_envelope_exists",
            format!("inbox envelope already exists: {}", target.display()),
        ));
    }
    let source_name = envelope
        .source
        .file_name()
        .map(OsStr::to_os_string)
        .ok_or_else(|| {
            AppError::unexpected(
                "invalid_job_envelope",
                format!("inbox job {} has no source filename", envelope.id),
            )
        })?;
    fs::rename(&envelope.directory, &target).map_err(|error| {
        AppError::unexpected(
            "inbox_envelope_move_failed",
            format!(
                "unable to move inbox job {} to {}: {error}",
                envelope.id,
                target.display()
            ),
        )
    })?;
    envelope.directory = target;
    envelope.source = envelope.directory.join("material").join(source_name);
    Ok(envelope)
}

fn validate_recovered_order(envelopes: &[Envelope]) -> Result<(), AppError> {
    let mut sequences = BTreeSet::new();
    let mut processing = Vec::new();
    let mut queued = Vec::new();
    for envelope in envelopes {
        if !sequences.insert(envelope.receipt.sequence) {
            return Err(AppError::unexpected(
                "invalid_inbox_order",
                format!(
                    "duplicate active inbox sequence {}",
                    envelope.receipt.sequence
                ),
            ));
        }
        match envelope.receipt.state.as_str() {
            "queued" => queued.push(envelope.receipt.sequence),
            "processing" => processing.push(envelope.receipt.sequence),
            _ => {}
        }
    }
    if processing.len() > 1 {
        return Err(AppError::unexpected(
            "invalid_inbox_order",
            "more than one inbox job is in processing state",
        ));
    }
    if let (Some(processing), Some(queued)) =
        (processing.into_iter().next(), queued.into_iter().min())
        && processing > queued
    {
        return Err(AppError::unexpected(
            "invalid_inbox_order",
            "an inbox processing job is ordered behind a queued job",
        ));
    }
    Ok(())
}

fn refresh_queued(
    spool: &Spool,
    queue: &mut BTreeMap<(u64, String), Envelope>,
) -> Result<(), AppError> {
    let index = read_index(spool)?;
    for scanned in scan_envelopes_at(&spool.queued, &index, false)? {
        let key = (
            scanned.envelope.receipt.sequence,
            scanned.envelope.id.clone(),
        );
        if !queue.contains_key(&key) {
            insert_queued(queue, scanned.envelope)?;
        }
    }
    Ok(())
}

struct ScannedEnvelope {
    envelope: Envelope,
    stored_version: u32,
}

fn scan_envelopes_at(
    path: &Path,
    index: &QueueIndex,
    persist_repairs: bool,
) -> Result<Vec<ScannedEnvelope>, AppError> {
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
                if persist_repairs {
                    if material.exists() {
                        fs::remove_dir(&material)?;
                    }
                    fs::remove_dir(&directory)?;
                }
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
        let (receipt, stored_version) = if receipt_path.exists() {
            read_receipt(&receipt_path, &id, &source)?
        } else {
            let name = source.file_name().ok_or_else(|| {
                AppError::unexpected(
                    "invalid_job_envelope",
                    format!("source {} has no basename", source.display()),
                )
            })?;
            let key = URL_SAFE_NO_PAD.encode(name.as_bytes());
            let first_seen_at = match index.entries.get(&key) {
                Some(entry) => entry.first_seen_at.clone(),
                None => now()?,
            };
            let metadata = metadata_for_recovered_source(&source, &first_seen_at)?;
            let sequence = sequence_from_job_id(&id)?;
            let receipt = new_receipt(&id, sequence, name, &metadata)?;
            (receipt, 0)
        };
        validate_receipt(&receipt, &id, &source)?;
        let envelope = Envelope {
            id,
            directory,
            source,
            expected_identity: None,
            receipt,
        };
        envelopes.push(ScannedEnvelope {
            envelope,
            stored_version,
        });
    }
    Ok(envelopes)
}

fn read_receipt(path: &Path, id: &str, source: &Path) -> Result<(JobReceipt, u32), AppError> {
    let bytes = fs::read(path)?;
    let version = serde_json::from_slice::<StoredFormatVersion>(&bytes)
        .map_err(|error| invalid_receipt(path, &error))?
        .version;
    match version {
        RECEIPT_VERSION => serde_json::from_slice(&bytes)
            .map(|receipt| (receipt, RECEIPT_VERSION))
            .map_err(|error| invalid_receipt(path, &error)),
        2 => {
            let receipt: VersionTwoJobReceipt =
                serde_json::from_slice(&bytes).map_err(|error| invalid_receipt(path, &error))?;
            upgrade_version_two_receipt(receipt, id).map(|receipt| (receipt, 2))
        }
        1 => {
            let legacy: LegacyJobReceipt =
                serde_json::from_slice(&bytes).map_err(|error| invalid_receipt(path, &error))?;
            upgrade_legacy_receipt(legacy, id, source).map(|receipt| (receipt, 1))
        }
        _ => Err(AppError::unexpected(
            "invalid_job_receipt",
            format!("unsupported job receipt {}", path.display()),
        )),
    }
}

fn upgrade_version_two_receipt(
    receipt: VersionTwoJobReceipt,
    id: &str,
) -> Result<JobReceipt, AppError> {
    if receipt.version != 2 || receipt.id != id {
        return Err(AppError::unexpected(
            "invalid_job_receipt",
            format!("version 2 job receipt does not match envelope {id}"),
        ));
    }
    Ok(JobReceipt {
        version: RECEIPT_VERSION,
        id: receipt.id,
        sequence: sequence_from_job_id(id)?,
        original_name: receipt.original_name,
        original_name_base64: receipt.original_name_base64,
        state: receipt.state,
        attempts: receipt.attempts,
        delivery_key: receipt.delivery_key,
        ingestion_id: receipt.ingestion_id,
        source_size_bytes: receipt.source_size_bytes,
        source_created_at: receipt.source_created_at,
        source_modified_at: receipt.source_modified_at,
        first_seen_at: receipt.first_seen_at,
        claimed_at: receipt.claimed_at,
        started_at: receipt.started_at,
        completed_at: receipt.completed_at,
        source_sha256: receipt.source_sha256,
        work: receipt.work,
        reconciliation_id: receipt.reconciliation_id,
        model_run_token: receipt.model_run_token,
        result_status: receipt.result_status,
        result_revision: receipt.result_revision,
        last_error: receipt.last_error,
    })
}

fn sequence_from_job_id(id: &str) -> Result<u64, AppError> {
    let digits = id
        .strip_prefix('j')
        .and_then(|rest| rest.split('-').next())
        .filter(|digits| digits.len() == 20 && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| {
            AppError::unexpected(
                "invalid_job_envelope",
                format!("inbox job identifier is invalid: {id}"),
            )
        })?;
    let sequence = digits.parse::<u64>().map_err(|_| {
        AppError::unexpected(
            "invalid_job_envelope",
            format!("inbox job sequence is invalid: {id}"),
        )
    })?;
    if sequence == 0 {
        return Err(AppError::unexpected(
            "invalid_job_envelope",
            format!("inbox job sequence is invalid: {id}"),
        ));
    }
    Ok(sequence)
}

fn invalid_receipt(path: &Path, error: &serde_json::Error) -> AppError {
    AppError::unexpected(
        "invalid_job_receipt",
        format!("invalid job receipt {}: {error}", path.display()),
    )
}

fn upgrade_legacy_receipt(
    legacy: LegacyJobReceipt,
    id: &str,
    source: &Path,
) -> Result<JobReceipt, AppError> {
    let encoded_name = source
        .file_name()
        .map(|name| URL_SAFE_NO_PAD.encode(name.as_bytes()));
    let displayed_name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let optional_text_is_valid = [
        legacy.started_at.as_deref(),
        legacy.completed_at.as_deref(),
        legacy.source_sha256.as_deref(),
        legacy.work.as_deref(),
        legacy.model_run_token.as_deref(),
        legacy.result_status.as_deref(),
    ]
    .into_iter()
    .flatten()
    .all(|value| !value.trim().is_empty());
    let error_is_valid = legacy
        .last_error
        .as_ref()
        .is_none_or(|error| !error.code.trim().is_empty() && !error.message.trim().is_empty());
    let state_is_valid = match legacy.state.as_str() {
        "processing" => {
            legacy.completed_at.is_none()
                && legacy.result_status.is_none()
                && legacy.result_revision.is_none()
        }
        "done" => {
            legacy.completed_at.is_some()
                && legacy.last_error.is_none()
                && (matches!(
                    (legacy.result_status.as_deref(), legacy.result_revision),
                    (Some("applied"), Some(revision)) if revision > 0
                ) || matches!(
                    (legacy.result_status.as_deref(), legacy.result_revision),
                    (Some("recorded"), None)
                ))
        }
        "failed" => {
            legacy.completed_at.is_some()
                && legacy.result_status.is_none()
                && legacy.result_revision.is_none()
                && legacy.last_error.is_some()
        }
        _ => false,
    };
    let progress_is_safe = state_is_valid
        && optional_text_is_valid
        && error_is_valid
        && (legacy.source_sha256.is_some() == legacy.work.is_some())
        && (legacy.reconciliation_id.is_none() || legacy.work.is_some())
        && (legacy.model_run_token.is_none() || legacy.work.is_some())
        && legacy.reconciliation_id.is_none_or(|id| id > 0)
        && (legacy.state != "done" || legacy.reconciliation_id.is_some())
        && (legacy.attempts == 0) == legacy.started_at.is_none()
        && (legacy.last_error.is_none() || legacy.attempts > 0)
        && (legacy.state == "processing" || legacy.attempts > 0);
    if legacy.version != 1
        || legacy.id != id
        || encoded_name.as_deref() != Some(&legacy.original_name_base64)
        || displayed_name.as_deref() != Some(&legacy.original_name)
        || legacy.created_at.trim().is_empty()
        || !progress_is_safe
    {
        return Err(AppError::unexpected(
            "invalid_job_receipt",
            format!(
                "legacy job receipt {} does not match its envelope",
                source
                    .parent()
                    .and_then(Path::parent)
                    .unwrap_or(source)
                    .display()
            ),
        ));
    }
    let metadata = metadata_for_recovered_source(source, &legacy.created_at)?;
    let last_error = (legacy.state == "processing")
        .then_some(legacy.last_error)
        .flatten();
    Ok(JobReceipt {
        version: RECEIPT_VERSION,
        id: legacy.id,
        sequence: sequence_from_job_id(id)?,
        original_name: legacy.original_name,
        original_name_base64: legacy.original_name_base64,
        state: "processing".to_owned(),
        attempts: legacy.attempts,
        delivery_key: format!("inbox:{id}:{}", legacy.created_at),
        ingestion_id: None,
        source_size_bytes: metadata.source_size_bytes,
        source_created_at: metadata.source_created_at,
        source_modified_at: metadata.source_modified_at,
        first_seen_at: legacy.created_at.clone(),
        claimed_at: legacy.created_at,
        started_at: legacy.started_at,
        completed_at: None,
        source_sha256: legacy.source_sha256,
        work: legacy.work,
        reconciliation_id: legacy.reconciliation_id,
        model_run_token: legacy.model_run_token,
        result_status: None,
        result_revision: None,
        last_error,
    })
}

fn ensure_ingestion(library: &Path, envelope: &mut Envelope) -> Result<i64, AppError> {
    let connection = db::open_write(library)?;
    let metadata = ingestion::SourceMetadata {
        source_name: envelope.receipt.original_name.clone(),
        source_size_bytes: envelope.receipt.source_size_bytes,
        source_created_at: envelope.receipt.source_created_at.clone(),
        source_modified_at: envelope.receipt.source_modified_at.clone(),
        first_seen_at: envelope.receipt.first_seen_at.clone(),
    };
    let ingestion_id = ingestion::begin(
        &connection,
        &ingestion::NewIngestion {
            delivery_key: Some(&envelope.receipt.delivery_key),
            channel: "inbox",
            metadata: &metadata,
        },
    )?;
    if envelope
        .receipt
        .ingestion_id
        .is_some_and(|recorded| recorded != ingestion_id)
    {
        return Err(AppError::unexpected(
            "invalid_job_receipt",
            "job receipt refers to a different source delivery",
        ));
    }
    if envelope.receipt.ingestion_id.is_none() {
        envelope.receipt.ingestion_id = Some(ingestion_id);
        write_receipt(envelope)?;
    }
    Ok(ingestion_id)
}

fn metadata_for_recovered_source(
    source: &Path,
    first_seen_at: &str,
) -> Result<ingestion::SourceMetadata, AppError> {
    let metadata = fs::metadata(source)?;
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            AppError::unexpected(
                "invalid_job_envelope",
                format!("source {} has no filename", source.display()),
            )
        })?;
    metadata_from_filesystem(OsStr::new(&name), &metadata, first_seen_at)
}

fn metadata_from_filesystem(
    name: &OsStr,
    metadata: &fs::Metadata,
    first_seen_at: &str,
) -> Result<ingestion::SourceMetadata, AppError> {
    Ok(ingestion::SourceMetadata {
        source_name: name.to_string_lossy().into_owned(),
        source_size_bytes: Some(metadata.len()),
        source_created_at: metadata
            .created()
            .ok()
            .map(ingestion::format_system_time)
            .transpose()?,
        source_modified_at: metadata
            .modified()
            .ok()
            .map(ingestion::format_system_time)
            .transpose()?,
        first_seen_at: first_seen_at.to_owned(),
    })
}

fn new_receipt(
    id: &str,
    sequence: u64,
    name: &OsStr,
    metadata: &ingestion::SourceMetadata,
) -> Result<JobReceipt, AppError> {
    Ok(JobReceipt {
        version: RECEIPT_VERSION,
        id: id.to_owned(),
        sequence,
        original_name: name.to_string_lossy().into_owned(),
        original_name_base64: URL_SAFE_NO_PAD.encode(name.as_bytes()),
        state: "queued".to_owned(),
        attempts: 0,
        delivery_key: format!("inbox:{id}:{}", metadata.first_seen_at),
        ingestion_id: None,
        source_size_bytes: metadata.source_size_bytes,
        source_created_at: metadata.source_created_at.clone(),
        source_modified_at: metadata.source_modified_at.clone(),
        first_seen_at: metadata.first_seen_at.clone(),
        claimed_at: now()?,
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
    let expected_sequence = sequence_from_job_id(id)?;
    let state_is_valid = match receipt.state.as_str() {
        "queued" => receipt_has_no_processing_progress(receipt),
        "processing" => receipt.completed_at.is_none() && receipt.result_status.is_none(),
        "done" => receipt.completed_at.is_some() && receipt.result_status.is_some(),
        "failed" => receipt.completed_at.is_some() && receipt.last_error.is_some(),
        _ => false,
    };
    if receipt.version != RECEIPT_VERSION
        || receipt.id != id
        || receipt.sequence != expected_sequence
        || encoded_name.as_deref() != Some(&receipt.original_name_base64)
        || receipt.delivery_key.trim().is_empty()
        || receipt.sequence == 0
        || receipt.first_seen_at.trim().is_empty()
        || receipt.claimed_at.trim().is_empty()
        || !state_is_valid
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
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.file_type().is_file() {
            ignored += 1;
            continue;
        }
        let identity = FileIdentity::from(&metadata);
        let key = URL_SAFE_NO_PAD.encode(name.as_os_str().as_bytes());
        visible.insert(key.clone());
        let queue_entry = if let Some(queue_entry) = index
            .entries
            .get(&key)
            .filter(|queue_entry| queue_entry.identity == identity)
        {
            queue_entry.clone()
        } else {
            let sequence = index.next_sequence;
            index.next_sequence = index.next_sequence.checked_add(1).ok_or_else(|| {
                AppError::unexpected("inbox_sequence_overflow", "inbox sequence is exhausted")
            })?;
            let queue_entry = QueueEntry {
                sequence,
                first_seen_at: now()?,
                identity,
            };
            index.entries.insert(key.clone(), queue_entry.clone());
            queue_entry
        };
        files.push(IncomingFile {
            path: entry.path(),
            metadata: metadata_from_filesystem(&name, &metadata, &queue_entry.first_seen_at)?,
            name,
            key,
            sequence: queue_entry.sequence,
            identity,
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

fn read_index(spool: &Spool) -> Result<QueueIndex, AppError> {
    let path = &spool.index;
    if !path.exists() {
        return Ok(QueueIndex::default());
    }
    let bytes = fs::read(path)?;
    let version = serde_json::from_slice::<StoredFormatVersion>(&bytes)
        .map_err(|error| invalid_index(path, &error))?
        .version;
    match version {
        QUEUE_VERSION => {
            let index: QueueIndex =
                serde_json::from_slice(&bytes).map_err(|error| invalid_index(path, &error))?;
            if index.next_sequence == 0 {
                return Err(unsupported_index(path));
            }
            Ok(index)
        }
        3 => {
            let mut index: QueueIndex =
                serde_json::from_slice(&bytes).map_err(|error| invalid_index(path, &error))?;
            if index.next_sequence == 0 {
                return Err(unsupported_index(path));
            }
            index.version = QUEUE_VERSION;
            Ok(index)
        }
        1 => {
            let legacy: LegacyQueueIndex =
                serde_json::from_slice(&bytes).map_err(|error| invalid_index(path, &error))?;
            upgrade_legacy_index(spool, legacy)
        }
        _ => Err(unsupported_index(path)),
    }
}

fn repair_sequence_high_water(spool: &Spool, index: &mut QueueIndex) -> Result<(), AppError> {
    let mut maximum = index
        .entries
        .values()
        .map(|entry| entry.sequence)
        .max()
        .unwrap_or(0);
    for directory in [
        &spool.queued,
        &spool.processing,
        &spool.done,
        &spool.duplicates,
        &spool.failed,
    ] {
        if !directory.exists() {
            continue;
        }
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            maximum = maximum.max(sequence_from_job_id(&id)?);
        }
    }
    let required = maximum.checked_add(1).ok_or_else(|| {
        AppError::unexpected("inbox_sequence_overflow", "inbox sequence is exhausted")
    })?;
    index.next_sequence = index.next_sequence.max(required);
    index.version = QUEUE_VERSION;
    Ok(())
}

fn upgrade_legacy_index(spool: &Spool, legacy: LegacyQueueIndex) -> Result<QueueIndex, AppError> {
    if legacy.version != 1 || legacy.next_sequence == 0 {
        return Err(unsupported_index(&spool.index));
    }
    let first_seen_at = now()?;
    let mut sequences = BTreeSet::new();
    let mut entries = BTreeMap::new();
    for (key, sequence) in legacy.entries {
        if sequence == 0 || sequence >= legacy.next_sequence || !sequences.insert(sequence) {
            return Err(unsupported_index(&spool.index));
        }
        let decoded = URL_SAFE_NO_PAD
            .decode(key.as_bytes())
            .map_err(|_| unsupported_index(&spool.index))?;
        if URL_SAFE_NO_PAD.encode(&decoded) != key {
            return Err(unsupported_index(&spool.index));
        }
        let name = OsStr::from_bytes(&decoded);
        if name.is_empty() || Path::new(name).file_name() != Some(name) || hidden_or_temporary(name)
        {
            return Err(unsupported_index(&spool.index));
        }
        let source = spool.incoming.join(name);
        let metadata = match fs::symlink_metadata(&source) {
            Ok(metadata) if metadata.file_type().is_file() => metadata,
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        entries.insert(
            key,
            QueueEntry {
                sequence,
                first_seen_at: first_seen_at.clone(),
                identity: FileIdentity::from(&metadata),
            },
        );
    }
    Ok(QueueIndex {
        version: QUEUE_VERSION,
        next_sequence: legacy.next_sequence,
        entries,
    })
}

fn invalid_index(path: &Path, error: &serde_json::Error) -> AppError {
    AppError::unexpected(
        "invalid_inbox_index",
        format!("invalid inbox index {}: {error}", path.display()),
    )
}

fn unsupported_index(path: &Path) -> AppError {
    AppError::unexpected(
        "invalid_inbox_index",
        format!("unsupported inbox index {}", path.display()),
    )
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
        queued: count_directories(&spool.queued)?,
        next_job: next_queued_job(spool)?,
        processing: count_directories(&spool.processing)?,
        done: count_directories(&spool.done)?,
        duplicates: count_directories(&spool.duplicates)?,
        failed: count_directories(&spool.failed)?,
        locked: inbox_locked(&spool.lock),
        paused: spool.pause_requested()?,
        maintenance: spool.maintenance_requested()?,
    })
}

fn next_queued_job(spool: &Spool) -> Result<Option<RegisteredJob>, AppError> {
    if !spool.queued.exists() {
        return Ok(None);
    }
    let mut next: Option<(u64, String, PathBuf)> = None;
    for entry in fs::read_dir(&spool.queued)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let sequence = sequence_from_job_id(&id)?;
        let candidate = (sequence, id, entry.path());
        if next
            .as_ref()
            .is_none_or(|current| (&candidate.0, &candidate.1) < (&current.0, &current.1))
        {
            next = Some(candidate);
        }
    }
    let Some((_, id, directory)) = next else {
        return Ok(None);
    };
    let source = sole_material(&directory.join("material"))?.ok_or_else(|| {
        AppError::unexpected(
            "invalid_job_envelope",
            format!("queued inbox job {id} has no source material"),
        )
    })?;
    let (receipt, _) = read_receipt(&directory.join("job.json"), &id, &source)?;
    validate_receipt(&receipt, &id, &source)?;
    let envelope = Envelope {
        id,
        directory,
        source,
        expected_identity: None,
        receipt,
    };
    Ok(Some(RegisteredJob::from(&envelope)))
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
        FileIdentity, QUEUE_VERSION, QueueIndex, Spool, backlog_sources, import_backlog_source,
        path_has_identity, read_index, register_settled_locked, scan_envelopes_at, scan_incoming,
        write_index,
    };

    #[test]
    fn empty_version_one_queue_index_is_upgraded_and_persisted()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let spool = Spool::new(directory.path());
        spool.create()?;
        fs::write(
            &spool.index,
            r#"{"version":1,"next_sequence":42,"entries":{}}"#,
        )?;

        let index = read_index(&spool)?;
        assert_eq!(index.version, QUEUE_VERSION);
        assert_eq!(index.next_sequence, 42);

        register_settled_locked(&spool, 0)?;
        let persisted: QueueIndex = serde_json::from_slice(&fs::read(&spool.index)?)?;
        assert_eq!(persisted.version, QUEUE_VERSION);
        assert_eq!(persisted.next_sequence, 42);
        assert!(persisted.entries.is_empty());
        Ok(())
    }

    #[test]
    fn version_three_queue_index_upgrades_before_registration()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let spool = Spool::new(directory.path());
        spool.create()?;
        fs::write(
            &spool.index,
            r#"{"version":3,"next_sequence":7,"entries":{}}"#,
        )?;

        let index = read_index(&spool)?;
        assert_eq!(index.version, QUEUE_VERSION);
        assert_eq!(index.next_sequence, 7);
        register_settled_locked(&spool, 0)?;
        let persisted: QueueIndex = serde_json::from_slice(&fs::read(&spool.index)?)?;
        assert_eq!(persisted.version, QUEUE_VERSION);
        assert_eq!(persisted.next_sequence, 7);
        Ok(())
    }

    #[test]
    fn registration_repairs_sequence_above_existing_envelopes()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let spool = Spool::new(directory.path());
        spool.create()?;
        fs::create_dir_all(spool.done.join("j00000000000000000042"))?;
        fs::write(spool.incoming.join("next.txt"), "next")?;

        let registered = register_settled_locked(&spool, 0)?;
        assert_eq!(registered.len(), 1);
        assert_eq!(registered[0].id, "j00000000000000000043");
        let persisted: QueueIndex = serde_json::from_slice(&fs::read(&spool.index)?)?;
        assert_eq!(persisted.next_sequence, 44);
        Ok(())
    }

    #[test]
    fn backlog_import_resets_envelopes_in_original_fifo_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let source_directory = tempfile::tempdir()?;
        let source = Spool::new(source_directory.path());
        source.create()?;
        fs::write(source.incoming.join("later-name.txt"), "first")?;
        let first = register_settled_locked(&source, 0)?;
        fs::rename(&first[0].directory, source.processing.join(&first[0].id))?;
        fs::write(source.incoming.join("earlier-name.txt"), "second")?;
        register_settled_locked(&source, 0)?;
        fs::write(source.incoming.join("late-arrival.txt"), "third")?;

        let destination_directory = tempfile::tempdir()?;
        let destination = Spool::new(destination_directory.path());
        destination.create()?;
        let backlog = backlog_sources(&source)?;
        assert_eq!(backlog.len(), 3);
        for (offset, item) in backlog.iter().enumerate() {
            import_backlog_source(&destination, item, u64::try_from(offset)? + 1)?;
        }

        let imported = scan_envelopes_at(&destination.queued, &QueueIndex::default(), false)?;
        let by_sequence = imported
            .into_iter()
            .map(|item| {
                Ok((
                    item.envelope.receipt.sequence,
                    fs::read_to_string(item.envelope.source)?,
                    item.envelope.receipt.attempts,
                ))
            })
            .collect::<Result<Vec<_>, std::io::Error>>()?;
        let mut by_sequence = by_sequence;
        by_sequence.sort_by_key(|item| item.0);
        assert_eq!(
            by_sequence,
            vec![
                (1, "first".to_owned(), 0),
                (2, "second".to_owned(), 0),
                (3, "third".to_owned(), 0),
            ]
        );
        assert_eq!(fs::read_to_string(&backlog[0].path)?, "first");
        Ok(())
    }

    #[test]
    fn settling_version_one_queue_entry_keeps_its_sequence_when_upgraded()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let spool = Spool::new(directory.path());
        spool.create()?;
        fs::write(spool.incoming.join("settling.txt"), "settling")?;
        let legacy = r#"{
  "version": 1,
  "next_sequence": 42,
  "entries": {
    "c2V0dGxpbmcudHh0": 41
  }
}"#;
        fs::write(&spool.index, legacy)?;

        let index = read_index(&spool)?;
        let entry = index
            .entries
            .get("c2V0dGxpbmcudHh0")
            .ok_or("settling legacy entry was lost")?;
        assert_eq!(entry.sequence, 41);
        assert!(!entry.first_seen_at.is_empty());
        assert!(register_settled_locked(&spool, 3_600)?.is_empty());

        let persisted: QueueIndex = serde_json::from_slice(&fs::read(&spool.index)?)?;
        assert_eq!(persisted.version, QUEUE_VERSION);
        assert_eq!(persisted.next_sequence, 42);
        assert_eq!(persisted.entries["c2V0dGxpbmcudHh0"].sequence, 41);

        let queued = register_settled_locked(&spool, 0)?;
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].id, "j00000000000000000041");
        Ok(())
    }

    #[test]
    fn malformed_version_one_queue_sequence_is_rejected() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let spool = Spool::new(directory.path());
        spool.create()?;
        fs::write(spool.incoming.join("settling.txt"), "settling")?;
        fs::write(
            &spool.index,
            r#"{"version":1,"next_sequence":41,"entries":{"c2V0dGxpbmcudHh0":41}}"#,
        )?;

        let Err(error) = read_index(&spool) else {
            return Err("malformed legacy index was accepted".into());
        };
        assert_eq!(error.code(), "invalid_inbox_index");
        Ok(())
    }

    #[test]
    fn pause_marker_rejects_symlinks() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let spool = Spool::new(directory.path());
        spool.create()?;
        let target = directory.path().join("target");
        fs::write(&target, "paused")?;
        symlink(&target, &spool.paused)?;

        let Err(error) = spool.pause_requested() else {
            return Err("symlink pause marker was accepted".into());
        };
        assert_eq!(error.code(), "inbox_pause_invalid");
        Ok(())
    }

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
        let mut index = read_index(&spool)?;
        let (mut second, _) = scan_incoming(&spool, &mut index, 0)?;
        second.sort_by_key(|entry| entry.sequence);
        assert_eq!(first[0].sequence, second[0].sequence);
        assert_eq!(second[0].name, "b.txt");
        assert_eq!(second[1].name, "a.txt");
        Ok(())
    }

    #[test]
    fn queue_index_restarts_replaced_path_identity() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let spool = Spool::new(directory.path());
        spool.create()?;
        let source = spool.incoming.join("source.txt");
        fs::write(&source, "first")?;
        let mut index = QueueIndex::default();
        scan_incoming(&spool, &mut index, 3_600)?;

        let key = index
            .entries
            .keys()
            .next()
            .cloned()
            .ok_or("queue index omitted the source")?;
        let original = index
            .entries
            .get_mut(&key)
            .ok_or("queue index omitted the source")?;
        let original_sequence = original.sequence;
        let original_identity = original.identity;
        original.first_seen_at = "2000-01-01T00:00:00Z".to_owned();

        let replacement = directory.path().join("replacement.txt");
        fs::write(&replacement, "second")?;
        fs::rename(&replacement, &source)?;
        let (rescanned, _) = scan_incoming(&spool, &mut index, 3_600)?;
        let replaced = index
            .entries
            .get(&key)
            .ok_or("queue index omitted the replacement")?;

        assert_eq!(rescanned.len(), 1);
        assert_ne!(replaced.identity, original_identity);
        assert_ne!(replaced.sequence, original_sequence);
        assert_ne!(replaced.first_seen_at, "2000-01-01T00:00:00Z");
        assert_eq!(rescanned[0].sequence, replaced.sequence);
        assert_eq!(rescanned[0].metadata.first_seen_at, replaced.first_seen_at);
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
