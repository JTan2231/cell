use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, TransactionBehavior};
use serde::Serialize;
use serde_json::{Value, json};

use crate::change::{
    ChangeOperation, ConceptSelector, EvidenceDisposition, EvidenceSelector, Reconciliation,
};
use crate::cli::{
    ChangeCommand, ChangeSelectArgs, ChangeShowArgs, Cli, CliGraphDirection, CliLibraryKind,
    Command, ConceptCommand, ConceptPageArgs, ConceptShowArgs, DecisionFeedCommand, GraphArgs,
    InboxCommand, InboxRetryCommand, InitArgs, IntegrateArgs, LatelyArgs, PagedAtArgs, SearchArgs,
    ShakeArgs, WorkAddArgs, WorkCommand,
};
use crate::config::Config;
use crate::corpus::{
    ReconciliationRecord, ShakePlan, StoredWork, apply_shake, diff, get_work, head_snapshot,
    list_commits, list_reconciliations, list_works, plan_shake, reconciliation_view,
    recorded_change_at, revision, select_reconciliation, store_ingested_work,
    store_retained_ingested_work, work_view,
};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::graph::{GraphReader, NeighborDirection};
use crate::model::{
    ConceptReference, ConceptSummary, DiffEntry, GraphDirection, GraphView, LibraryStats, Page,
    PageInfo,
};
use crate::model_runner::{ModelSettings, Runner};
use crate::render::{CommandOutput, render_terminal_text};
use crate::resolver::{ResolvedEvidence, ResolvedOperation};
use crate::{decision_feed, inbox, ingestion, liaison, resolver};

pub fn selected_library_path(cli: &Cli, config: &Config) -> Result<PathBuf, AppError> {
    if matches!(
        cli.command,
        Command::DecisionFeed(_) | Command::Inbox(InboxCommand::Accept(_))
    ) {
        if cli.config.is_none() {
            return Err(AppError::invalid(
                "decision_feed_config_required",
                "decision-account acceptance and feed reads require an explicit --config",
            ));
        }
        if cli.library.is_some() {
            return Err(AppError::invalid(
                "decision_feed_library_override",
                "decision-account acceptance and feed reads do not permit --library",
            ));
        }
        config.decision_feed()?;
        return config.library.clone().ok_or_else(|| {
            AppError::invalid(
                "decision_feed_library_not_configured",
                "the selected decision-feed configuration must define library",
            )
        });
    }
    library_path(cli.library.as_ref(), config)
}

pub fn library_path(explicit: Option<&PathBuf>, config: &Config) -> Result<PathBuf, AppError> {
    let environment = std::env::var_os("ANNALS_LIBRARY");
    resolve_library_path(
        explicit.map(PathBuf::as_path),
        environment.as_deref(),
        config.library.as_deref(),
    )
}

fn resolve_library_path(
    explicit: Option<&Path>,
    environment: Option<&OsStr>,
    configured: Option<&Path>,
) -> Result<PathBuf, AppError> {
    explicit
        .map(Path::to_path_buf)
        .or_else(|| {
            environment
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| configured.map(Path::to_path_buf))
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| {
            AppError::invalid(
                "library_not_configured",
                "no library is configured; pass --library, set ANNALS_LIBRARY, or select a configuration that defines library",
            )
        })
}

pub fn run(cli: &Cli, config: &Config, path: &Path) -> AppResult<CommandOutput> {
    match &cli.command {
        Command::Init(args) => initialize(path, args),
        Command::Migrate => migrate_library(path),
        Command::Stats => stats(path),
        Command::Overview(args) => overview(path, args.at),
        Command::Roots(args) => roots(path, args),
        Command::Concept(command) => match command {
            ConceptCommand::Show(args) => show_concept(path, args),
            ConceptCommand::Parents(args) => concept_neighbors(path, args, GraphDirection::Parents),
            ConceptCommand::Children(args) => {
                concept_neighbors(path, args, GraphDirection::Children)
            }
            ConceptCommand::Evidence(args) => concept_evidence(path, args),
        },
        Command::Graph(args) => graph(path, args),
        Command::Shake(args) => shake(path, args, cli.json),
        Command::Backup(args) => backup(path, &args.output),
        Command::Work(command) => match command {
            WorkCommand::Add(args) => {
                reject_direct_decision_ingress(config)?;
                db::require_path_kind(path, db::LibraryKind::General)?;
                add_work(path, args)
            }
            WorkCommand::List => work_list(path),
            WorkCommand::Show(args) => show_work(path, &args.label),
        },
        Command::Integrate(args) => {
            reject_direct_decision_ingress(config)?;
            db::require_path_kind(path, db::LibraryKind::General)?;
            integrate(path, config, args, !cli.json)
        }
        Command::Inbox(command) => {
            let expected = if config.decision_feed.is_some() {
                db::LibraryKind::Decisions
            } else {
                db::LibraryKind::General
            };
            db::require_path_kind(path, expected)?;
            match command {
                InboxCommand::Run(args) => inbox::run(path, config, args, !cli.json),
                InboxCommand::Register(args) => inbox::register(config, args),
                InboxCommand::Enqueue(args) => inbox::enqueue(config, args),
                InboxCommand::Accept(args) => inbox::accept(path, config, args),
                InboxCommand::Prioritize(args) => inbox::prioritize(config, args),
                InboxCommand::Deprioritize(args) => inbox::deprioritize(config, args),
                InboxCommand::ImportBacklog(args) => inbox::import_backlog(config, &args.from),
                InboxCommand::Pause => inbox::pause(config),
                InboxCommand::Resume => inbox::resume(path, config),
                InboxCommand::Interrupt(args) => inbox::interrupt(path, config, args),
                InboxCommand::Retry(command) => match command {
                    InboxRetryCommand::Preview(args) => inbox::retry_preview(path, config, args),
                    InboxRetryCommand::Start(args) => {
                        inbox::retry_start(path, config, args, !cli.json)
                    }
                    InboxRetryCommand::Status(args) => inbox::retry_status(path, args),
                    InboxRetryCommand::Continue(args) => {
                        inbox::retry_continue(path, config, args, !cli.json)
                    }
                },
                InboxCommand::Status => inbox::status(path, config),
            }
        }
        Command::DecisionFeed(command) => match command {
            DecisionFeedCommand::Watermark => decision_feed::watermark(path, config),
            DecisionFeedCommand::Page(args) => decision_feed::page(path, config, args),
        },
        Command::Change(command) => match command {
            ChangeCommand::Submit(args) => submit_change(path, &args.input, &args.work, args.base),
            ChangeCommand::Show(args) => show_change(path, args),
            ChangeCommand::Validate(args) => validate_change(path, args),
            ChangeCommand::Apply(args) => apply_change(path, args),
            ChangeCommand::List => change_list(path),
        },
        Command::Search(args) => search(path, args),
        Command::Lately(args) => lately(path, args),
        Command::Log(args) => log(path, args.limit),
        Command::Diff(args) => diff_revisions(path, args.from, args.to),
        Command::Revert(args) => revert(path, args.revision),
    }
}

fn reject_direct_decision_ingress(config: &Config) -> Result<(), AppError> {
    if config.decision_feed.is_some() {
        Err(AppError::conflict(
            "decision_feed_accept_required",
            "a decisions library admits source material only through inbox accept --producer krisis",
        ))
    } else {
        Ok(())
    }
}

fn initialize(path: &Path, args: &InitArgs) -> Result<CommandOutput, AppError> {
    let kind = match args.kind {
        CliLibraryKind::General => db::LibraryKind::General,
        CliLibraryKind::Decisions => db::LibraryKind::Decisions,
    };
    let connection = match kind {
        db::LibraryKind::General => db::init(path)?,
        db::LibraryKind::Decisions => db::init_with_kind(path, kind)?,
    };
    let library_id = decision_feed::library_id(&connection)?;
    Ok(CommandOutput::new(
        json!({
            "library": path.display().to_string(),
            "library_id": library_id,
            "kind": kind.as_str(),
            "revision": 0
        }),
        format!(
            "Initialized Annals library {} at revision 0",
            path.display()
        ),
    )
    .mutation())
}

fn migrate_library(path: &Path) -> Result<CommandOutput, AppError> {
    let result = db::migrate(path)?;
    let human = if result.migrated {
        format!(
            "Migrated {} from schema version {} to {}",
            path.display(),
            result.from_version,
            result.to_version
        )
    } else {
        format!(
            "Library {} is already at schema version {}",
            path.display(),
            result.to_version
        )
    };
    Ok(CommandOutput::new(
        json!({
            "library": path.display().to_string(),
            "from_version": result.from_version,
            "to_version": result.to_version,
            "migrated": result.migrated,
        }),
        human,
    )
    .mutation())
}

fn stats(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let state = head_snapshot(&connection)?;
    let value = LibraryStats {
        revision: revision(&connection)?,
        concept_count: u64::try_from(state.concepts.len()).map_err(|_| stats_overflow())?,
        edge_count: u64::try_from(state.edges.len()).map_err(|_| stats_overflow())?,
        work_count: count(&connection, "works")?,
        ingestion_count: count(&connection, "ingestions")?,
        evidence_count: u64::try_from(state.evidence.len()).map_err(|_| stats_overflow())?,
        pending_reconciliation_count: count_where(
            &connection,
            "reconciliations",
            "status = 'pending'",
        )?,
        commit_count: count(&connection, "commits")?,
        model_run_count: count(&connection, "model_runs")?,
        database_size_bytes: fs::metadata(path).map(|metadata| metadata.len()).map_err(
            |error| {
                AppError::unexpected(
                    "database_metadata_failed",
                    format!("unable to inspect {}: {error}", path.display()),
                )
            },
        )?,
    };
    let human = format!(
        "Revision: {}\nConcepts: {}\nParent edges: {}\nWorks: {}\nSource deliveries: {}\nEvidence links: {}\nPending reconciliations: {}\nCommits: {}\nModel runs: {}\nDatabase size: {} bytes",
        value.revision,
        value.concept_count,
        value.edge_count,
        value.work_count,
        value.ingestion_count,
        value.evidence_count,
        value.pending_reconciliation_count,
        value.commit_count,
        value.model_run_count,
        value.database_size_bytes
    );
    Ok(CommandOutput::new(to_value(&value)?, human))
}

fn backup(path: &Path, output: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_backup_source(path)?;
    db::backup(&connection, output)?;
    Ok(CommandOutput::new(
        json!({ "output": output.display().to_string() }),
        format!("Backed up {} to {}", path.display(), output.display()),
    )
    .mutation())
}

fn add_work(path: &Path, args: &WorkAddArgs) -> Result<CommandOutput, AppError> {
    let _manual_delivery_lock = acquire_manual_delivery_lock(path)?;
    let (retained, _) = ingest_manual_work(path, &args.input, args.name.as_deref(), true)?;
    let work = retained.work;
    let connection = db::open_read(path)?;
    let corpus_revision = revision(&connection)?;
    Ok(CommandOutput::new(
        json!({
            "work": work.label,
            "size_bytes": work.text.len(),
            "sha256": work.sha256,
            "first_retained_at": work.created_at,
            "retention": if retained.new_work { "new" } else { "duplicate" },
            "corpus_revision": corpus_revision
        }),
        format!(
            "{} work {:?} ({} bytes)\nCorpus remains at revision {corpus_revision}",
            if retained.new_work {
                "Retained new"
            } else {
                "Recognized existing"
            },
            work.label,
            work.text.len()
        ),
    )
    .mutation())
}

fn work_list(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let works = list_works(&connection)?;
    let human = if works.is_empty() {
        "No retained works".to_owned()
    } else {
        works
            .iter()
            .map(|work| {
                format!(
                    "{}\t{} bytes",
                    render_terminal_text(&work.work, false),
                    work.size_bytes
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(to_value(&works)?, human))
}

fn show_work(path: &Path, label: &str) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let work = get_work(&connection, label)?;
    let view = work_view(&work);
    let data = json!({
        "work": view.summary.work,
        "size_bytes": view.summary.size_bytes,
        "sha256": view.summary.sha256,
        "first_retained_at": view.summary.first_retained_at,
        "headings": view.headings,
        "text": work.text
    });
    let human = format!(
        "Work: {}\nSize: {} bytes\nSHA-256: {}\nFirst retained: {}\n\n{}",
        render_terminal_text(&work.label, false),
        work.text.len(),
        work.sha256,
        work.created_at,
        render_terminal_text(&work.text, true)
    );
    Ok(CommandOutput::new(data, human))
}

fn integrate(
    path: &Path,
    config: &Config,
    args: &IntegrateArgs,
    forward_progress: bool,
) -> Result<CommandOutput, AppError> {
    let _manual_delivery_lock = args
        .input
        .as_ref()
        .map(|_| acquire_manual_delivery_lock(path))
        .transpose()?;
    let (work, ingestion_id) = if let Some(label) = &args.work {
        let connection = db::open_read(path)?;
        (get_work(&connection, label)?, None)
    } else {
        let input = args.input.as_ref().ok_or_else(|| {
            AppError::invalid("invalid_command", "integrate requires an input or --work")
        })?;
        let (retained, ingestion_id) =
            ingest_manual_work(path, input, args.name.as_deref(), false)?;
        (retained.work, Some(ingestion_id))
    };
    let quality = args.quality.unwrap_or(config.liaison.quality);
    let model = args.model.as_deref().or(config.liaison.model.as_deref());
    let settings = ModelSettings::new(quality, model);
    let runner = Runner::for_socket(config.liaison.nucleus_socket.as_deref());
    let record = liaison::integrate_with_runner(
        path,
        &work,
        &settings,
        forward_progress,
        args.reexamine,
        &runner,
    );
    let record = match record {
        Ok(record) => record,
        Err(error) => {
            if let Some(ingestion_id) = ingestion_id {
                fail_ingestion(path, ingestion_id, &error)?;
            }
            return Err(error);
        }
    };
    if args.apply {
        match record.status.as_str() {
            "pending" => {
                let mut connection = db::open_write(path)?;
                let application = if let Some(ingestion_id) = ingestion_id {
                    resolver::apply_record_for_ingestion(&mut connection, &record, ingestion_id)
                } else {
                    resolver::apply_record(&mut connection, &record)
                };
                let applied = match application {
                    Ok(applied) => applied,
                    Err(error) => {
                        if let Some(ingestion_id) = ingestion_id {
                            ingestion::fail(&connection, ingestion_id, &error)?;
                        }
                        return Err(error);
                    }
                };
                return applied_output(path, &record, applied);
            }
            "recorded" => {
                if let Some(ingestion_id) = ingestion_id {
                    complete_ingestion(path, ingestion_id, "recorded", None)?;
                }
            }
            _ => {
                let error = AppError::conflict(
                    "nothing_to_apply",
                    "the reusable reconciliation is not pending; use --reexamine for a fresh examination",
                );
                if let Some(ingestion_id) = ingestion_id {
                    fail_ingestion(path, ingestion_id, &error)?;
                }
                return Err(error);
            }
        }
    } else if let Some(ingestion_id) = ingestion_id {
        match record.status.as_str() {
            "pending" => complete_ingestion(path, ingestion_id, "pending", None)?,
            "recorded" => complete_ingestion(path, ingestion_id, "recorded", None)?,
            "applied" => {
                complete_ingestion(path, ingestion_id, "applied", record.applied_revision)?;
            }
            _ => {
                let error = AppError::database(
                    "invalid_reconciliation",
                    format!("unknown reconciliation status {:?}", record.status),
                );
                fail_ingestion(path, ingestion_id, &error)?;
                return Err(error);
            }
        }
    }
    reconciliation_output(path, &record)
}

fn ingest_manual_work(
    path: &Path,
    input: &Path,
    name: Option<&str>,
    complete_retention: bool,
) -> Result<(StoredWork, i64), AppError> {
    let metadata = ingestion::SourceMetadata::manual(input, name)?;
    let mut connection = db::open_write(path)?;
    let ingestion_id = ingestion::begin(
        &connection,
        &ingestion::NewIngestion {
            delivery_key: None,
            channel: "manual",
            metadata: &metadata,
        },
    )?;
    let retained = (|| {
        let text = read_utf8(input, "work")?;
        let label = work_label(input, name)?;
        if complete_retention {
            store_retained_ingested_work(&mut connection, ingestion_id, &label, &text)
        } else {
            store_ingested_work(&mut connection, ingestion_id, &label, &text)
        }
    })();
    match retained {
        Ok(retained) => Ok((retained, ingestion_id)),
        Err(error) => {
            ingestion::fail(&connection, ingestion_id, &error)?;
            Err(error)
        }
    }
}

fn acquire_manual_delivery_lock(path: &Path) -> Result<File, AppError> {
    let connection = db::open_write(path)?;
    let mut lock_name = path.as_os_str().to_os_string();
    lock_name.push(".manual.lock");
    let lock_path = PathBuf::from(lock_name);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
        .map_err(|error| {
            AppError::unexpected(
                "manual_ingestion_lock_failed",
                format!("unable to open manual source-delivery lock: {error}"),
            )
        })?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|error| {
        if error.kind() == io::ErrorKind::WouldBlock {
            AppError::conflict(
                "manual_ingestion_busy",
                "another manual source delivery is being processed",
            )
        } else {
            AppError::unexpected(
                "manual_ingestion_lock_failed",
                format!("unable to lock manual source deliveries: {error}"),
            )
        }
    })?;
    ingestion::fail_interrupted_manual(&connection)?;
    Ok(file)
}

fn complete_ingestion(
    path: &Path,
    ingestion_id: i64,
    result: &str,
    revision: Option<i64>,
) -> Result<(), AppError> {
    let connection = db::open_write(path)?;
    ingestion::complete(&connection, ingestion_id, result, revision)
}

fn fail_ingestion(path: &Path, ingestion_id: i64, error: &AppError) -> Result<(), AppError> {
    let connection = db::open_write(path)?;
    ingestion::fail(&connection, ingestion_id, error)
}

fn submit_change(
    path: &Path,
    input: &Path,
    work_label: &str,
    base: i64,
) -> Result<CommandOutput, AppError> {
    let document = read_utf8(input, "change request")?;
    let mut connection = db::open_write(path)?;
    let work = get_work(&connection, work_label)?;
    let record = resolver::submit_document(&mut connection, &work, base, &document, "human", None)?;
    reconciliation_output(path, &record)
}

fn show_change(path: &Path, args: &ChangeShowArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    if let Some(requested_revision) = args.at {
        let change = recorded_change_at(&connection, requested_revision)?;
        let human = render_recorded_change(&change)?;
        return Ok(CommandOutput::new(to_value(&change)?, human));
    }
    let record = select_reconciliation(&connection, args.work.as_deref(), false)?;
    let mut output = reconciliation_output(path, &record)?;
    output.quietable = false;
    Ok(output)
}

fn render_recorded_change(change: &crate::model::RecordedChangeView) -> Result<String, AppError> {
    let work = change
        .work
        .as_deref()
        .map_or_else(|| "none".to_owned(), render_quoted);
    let details = match change.kind.as_str() {
        "change" => render_recorded_reconciliation(change)?,
        "revert" => render_recorded_revert(change)?,
        "shake" => render_recorded_shake(),
        _ => {
            return Err(AppError::database(
                "invalid_commit_kind",
                format!("revision {} has an unknown change kind", change.revision),
            ));
        }
    };
    let effects = render_commit_effects(&change.effects);
    Ok(format!(
        "Applied {} at revision {}\nWork: {}\nSummary: {}\n{}\n{}\nActor: {}\nRecorded: {}",
        change.kind,
        change.revision,
        work,
        render_terminal_text(&change.summary, false),
        details,
        effects,
        render_terminal_text(&change.actor, false),
        render_terminal_text(&change.created_at, false),
    ))
}

fn render_recorded_shake() -> String {
    "Submitted request: transitively reduce the concept graph".to_owned()
}

fn render_recorded_reconciliation(
    change: &crate::model::RecordedChangeView,
) -> Result<String, AppError> {
    let reconciliation: Reconciliation = serde_json::from_value(change.submitted_request.clone())
        .map_err(|error| {
        AppError::database(
            "invalid_commit_request",
            format!(
                "revision {} has an invalid reconciliation: {error}",
                change.revision
            ),
        )
    })?;
    let resolved: Vec<ResolvedOperation> =
        serde_json::from_value(change.resolved_operations.clone()).map_err(|error| {
            AppError::database(
                "invalid_commit_operations",
                format!(
                    "revision {} has invalid resolved operations: {error}",
                    change.revision
                ),
            )
        })?;
    Ok(format!(
        "Submitted operations ({}):\n{}\nResolved operations ({}):\n{}\n{}",
        reconciliation.operations().len(),
        render_requested_operations(reconciliation.operations()),
        resolved.len(),
        render_resolved_operations(&resolved, reconciliation.operations()),
        render_annotations(reconciliation.annotations()),
    ))
}

fn render_recorded_revert(change: &crate::model::RecordedChangeView) -> Result<String, AppError> {
    let target = change
        .submitted_request
        .get("revert_revision")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            AppError::database(
                "invalid_commit_request",
                format!("revision {} has an invalid revert request", change.revision),
            )
        })?;
    Ok(format!("Submitted request: revert revision {target}"))
}

fn render_commit_effects(effects: &[DiffEntry]) -> String {
    let rendered = effects
        .iter()
        .map(|entry| format!("  {}", render_diff_entry(entry)))
        .collect::<Vec<_>>()
        .join("\n");
    format!("Material effects ({}):\n{rendered}", effects.len())
}

fn validate_change(path: &Path, args: &ChangeSelectArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let record = select_reconciliation(&connection, args.work.as_deref(), true)?;
    let resolved = resolver::validate_record(&connection, &record)?;
    let reconciliation = crate::change::load_request(&connection, record.request_id)?;
    let human = format!(
        "Valid pending reconciliation for {}\nBase revision: {}\nSummary: {}\nResolved operations ({}):\n{}\n{}\nApplication: ready",
        render_quoted(&record.work_label),
        record.base_revision,
        render_terminal_text(&record.summary, false),
        resolved.operations.len(),
        render_resolved_operations(&resolved.operations, reconciliation.operations()),
        render_annotations(reconciliation.annotations()),
    );
    Ok(CommandOutput::new(
        json!({
            "work": record.work_label,
            "base_revision": record.base_revision,
            "status": "valid",
            "summary": record.summary,
            "operations": resolved.operations
        }),
        human,
    ))
}

fn apply_change(path: &Path, args: &ChangeSelectArgs) -> Result<CommandOutput, AppError> {
    let mut connection = db::open_write(path)?;
    let record = select_reconciliation(&connection, args.work.as_deref(), true)?;
    let applied = resolver::apply_record(&mut connection, &record)?;
    applied_output(path, &record, applied)
}

fn change_list(path: &Path) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let reconciliations = list_reconciliations(&connection)?;
    let human = if reconciliations.is_empty() {
        "No recorded reconciliations".to_owned()
    } else {
        reconciliations
            .iter()
            .map(|reconciliation| {
                format!(
                    "{}\t{}\t{}\t{}",
                    reconciliation.status,
                    render_terminal_text(&reconciliation.work, false),
                    reconciliation.base_revision,
                    render_terminal_text(&reconciliation.summary, false)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(to_value(&reconciliations)?, human))
}

fn reconciliation_output(
    path: &Path,
    record: &ReconciliationRecord,
) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let view = reconciliation_view(&connection, record)?;
    let reconciliation: Reconciliation = serde_json::from_value(view.request.clone())?;
    let operation_count = view.request["operations"].as_array().map_or(0, Vec::len);
    let data = json!({
        "work": view.work,
        "base_revision": view.base_revision,
        "status": view.status,
        "summary": view.summary,
        "operation_count": operation_count,
        "annotations": view.annotations,
        "reconciliation": view.request,
        "created_at": view.created_at,
        "applied_revision": view.applied_revision
    });
    let heading = match view.status.as_str() {
        "applied" => "Applied reconciliation",
        "superseded" => "Superseded reconciliation",
        "recorded" => "Reconciliation recorded",
        _ => "Pending reconciliation",
    };
    let status = match view.status.as_str() {
        "applied" => view.applied_revision.map_or_else(
            || "applied".to_owned(),
            |revision| format!("applied at revision {revision}"),
        ),
        "superseded" => "superseded".to_owned(),
        "recorded" => "recorded".to_owned(),
        _ => "valid".to_owned(),
    };
    let corpus_state = if view.status == "recorded" {
        format!("\nCorpus remained at revision {}", view.base_revision)
    } else {
        String::new()
    };
    let human = format!(
        "{heading} for {}\nBase revision: {}\nSummary: {}\nOperations ({}):\n{}\n{}\nStatus: {status}{corpus_state}",
        render_quoted(&view.work),
        view.base_revision,
        render_terminal_text(&view.summary, false),
        reconciliation.operations().len(),
        render_requested_operations(reconciliation.operations()),
        render_annotations(reconciliation.annotations()),
    );
    Ok(CommandOutput::new(data, human).mutation())
}

fn render_requested_operations(operations: &[ChangeOperation]) -> String {
    operations
        .iter()
        .enumerate()
        .flat_map(|(index, operation)| render_requested_operation(index, operation))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_requested_operation(index: usize, operation: &ChangeOperation) -> Vec<String> {
    let number = index + 1;
    match operation {
        ChangeOperation::CreateConcept {
            handle,
            label,
            parents,
            evidence,
        } => {
            let mut lines = vec![format!(
                "  {number}. Create concept {} as ref {}",
                render_quoted(label),
                render_terminal_text(handle, false)
            )];
            lines.push(format!("     Parents: {}", render_selectors(parents)));
            append_evidence_selectors(&mut lines, evidence);
            lines
        }
        ChangeOperation::AddParent { concept, parent } => vec![format!(
            "  {number}. Add parent {} to {}",
            render_selector(parent),
            render_selector(concept)
        )],
        ChangeOperation::RemoveParent { concept, parent } => vec![format!(
            "  {number}. Remove parent {} from {}",
            render_selector(parent),
            render_selector(concept)
        )],
        ChangeOperation::AddEvidence { concept, evidence } => {
            let mut lines = vec![format!(
                "  {number}. Add evidence to {}",
                render_selector(concept)
            )];
            append_evidence_selectors(&mut lines, evidence);
            lines
        }
        ChangeOperation::RemoveEvidence { concept, evidence } => {
            let mut lines = vec![format!(
                "  {number}. Remove evidence from {}",
                render_selector(concept)
            )];
            append_evidence_selectors(&mut lines, evidence);
            lines
        }
        ChangeOperation::RewordConcept {
            concept,
            label,
            evidence_disposition,
        } => vec![
            format!(
                "  {number}. Reword {} to {}",
                render_selector(concept),
                render_quoted(label)
            ),
            format!(
                "     Evidence disposition: {}",
                render_evidence_disposition(*evidence_disposition)
            ),
        ],
        ChangeOperation::RetireConcept {
            concept,
            replacement,
        } => vec![
            format!("  {number}. Retire {}", render_selector(concept)),
            format!(
                "     Replacement: {}",
                replacement
                    .as_ref()
                    .map_or_else(|| "none".to_owned(), render_selector,)
            ),
        ],
    }
}

fn render_resolved_operations(
    operations: &[ResolvedOperation],
    requested: &[ChangeOperation],
) -> String {
    operations
        .iter()
        .enumerate()
        .flat_map(|(index, operation)| {
            render_resolved_operation(index, operation, requested.get(index))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_resolved_operation(
    index: usize,
    operation: &ResolvedOperation,
    _requested: Option<&ChangeOperation>,
) -> Vec<String> {
    let number = index + 1;
    match operation {
        ResolvedOperation::CreateConcept {
            concept,
            parents,
            evidence,
        } => {
            let mut lines = vec![format!(
                "  {number}. Create concept {}",
                render_reference(concept)
            )];
            lines.push(format!("     Parents: {}", render_references(parents)));
            append_resolved_evidence(&mut lines, evidence);
            lines
        }
        ResolvedOperation::AddParent { concept, parent } => vec![format!(
            "  {number}. Add parent {} to {}",
            render_reference(parent),
            render_reference(concept)
        )],
        ResolvedOperation::RemoveParent { concept, parent } => vec![format!(
            "  {number}. Remove parent {} from {}",
            render_reference(parent),
            render_reference(concept)
        )],
        ResolvedOperation::AddEvidence { concept, evidence } => {
            let mut lines = vec![format!(
                "  {number}. Add evidence to {}",
                render_reference(concept)
            )];
            append_resolved_evidence(&mut lines, evidence);
            lines
        }
        ResolvedOperation::RemoveEvidence { concept, evidence } => {
            let mut lines = vec![format!(
                "  {number}. Remove evidence from {}",
                render_reference(concept)
            )];
            append_resolved_evidence(&mut lines, evidence);
            lines
        }
        ResolvedOperation::RewordConcept {
            id,
            before,
            after,
            evidence_disposition,
        } => vec![
            format!(
                "  {number}. Reword {id}: {} -> {}",
                render_quoted(before),
                render_quoted(after)
            ),
            format!(
                "     Evidence disposition: {}",
                render_evidence_disposition(*evidence_disposition)
            ),
        ],
        ResolvedOperation::RetireConcept {
            concept,
            replacement,
            removed_parents,
            removed_children,
        } => vec![
            format!("  {number}. Retire {}", render_reference(concept)),
            format!(
                "     Replacement: {}",
                replacement
                    .as_ref()
                    .map_or_else(|| "none".to_owned(), render_reference)
            ),
            format!(
                "     Removed parents: {}",
                render_references(removed_parents)
            ),
            format!(
                "     Removed children: {}",
                render_references(removed_children)
            ),
        ],
    }
}

fn render_selectors(selectors: &[ConceptSelector]) -> String {
    if selectors.is_empty() {
        return "none (root)".to_owned();
    }
    selectors
        .iter()
        .map(render_selector)
        .collect::<Vec<_>>()
        .join(", ")
}

fn append_evidence_selectors(lines: &mut Vec<String>, evidence: &[EvidenceSelector]) {
    for selector in evidence {
        lines.push(format!("     Evidence: {}", render_quoted(&selector.quote)));
        if let Some(path) = &selector.within_heading {
            lines.push(format!("       Within heading: {}", render_path(path)));
        }
        for (label, value) in [
            ("Preceded by", selector.preceded_by.as_deref()),
            ("Followed by", selector.followed_by.as_deref()),
        ] {
            if let Some(value) = value {
                lines.push(format!("       {label}: {}", render_quoted(value)));
            }
        }
    }
}

fn append_resolved_evidence(lines: &mut Vec<String>, evidence: &[ResolvedEvidence]) {
    for item in evidence {
        let occurrences = if item.occurrence_count == 1 {
            String::new()
        } else {
            format!(" ({} occurrences)", item.occurrence_count)
        };
        lines.push(format!(
            "     Evidence: {}{occurrences}",
            render_quoted(&item.quote)
        ));
    }
}

fn render_selector(selector: &ConceptSelector) -> String {
    match selector {
        ConceptSelector::Existing { id } => id.to_string(),
        ConceptSelector::New { handle } => {
            format!("new ref {}", render_quoted(handle))
        }
    }
}

fn render_reference(reference: &ConceptReference) -> String {
    format!("{} ({})", render_quoted(&reference.label), reference.id)
}

fn render_references(references: &[ConceptReference]) -> String {
    if references.is_empty() {
        "none".to_owned()
    } else {
        references
            .iter()
            .map(render_reference)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn render_path(path: &[String]) -> String {
    if path.is_empty() {
        "root".to_owned()
    } else {
        path.iter()
            .map(|segment| render_quoted(segment))
            .collect::<Vec<_>>()
            .join(" › ")
    }
}

fn render_quoted(text: &str) -> String {
    format!("“{}”", render_terminal_text(text, false))
}

fn render_evidence_disposition(disposition: EvidenceDisposition) -> &'static str {
    match disposition {
        EvidenceDisposition::Retain => "retain existing evidence",
        EvidenceDisposition::Remove => "remove existing evidence",
    }
}

fn render_annotations(annotations: &[String]) -> String {
    if annotations.is_empty() {
        "Annotations: none".to_owned()
    } else {
        format!(
            "Annotations:\n{}",
            annotations
                .iter()
                .map(|annotation| { format!("  - {}", render_terminal_text(annotation, false)) })
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

fn applied_output(
    path: &Path,
    record: &ReconciliationRecord,
    applied: i64,
) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let request = crate::change::load_request(&connection, record.request_id)?;
    let reconciliation = serde_json::to_value(&request)?;
    Ok(CommandOutput::new(
        json!({
            "work": record.work_label,
            "base_revision": record.base_revision,
            "revision": applied,
            "status": "applied",
            "summary": record.summary,
            "annotations": request.annotations(),
            "reconciliation": reconciliation
        }),
        format!(
            "Applied reconciliation at revision {applied}:\n{}\n{}",
            render_terminal_text(&record.summary, false),
            render_annotations(request.annotations())
        ),
    )
    .mutation())
}

fn overview(path: &Path, requested: Option<i64>) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let reader = GraphReader::new(&connection);
    let graph = requested.map_or_else(|| reader.head(), |revision| reader.at(revision))?;
    let value = graph.overview()?;
    let human = format!(
        "Corpus revision {}\n{} concepts · {} scope edges\n{} roots · {} leaves · {} shared concepts\n{} evidence links",
        value.revision,
        value.concept_count,
        value.edge_count,
        value.root_count,
        value.leaf_count,
        value.shared_concept_count,
        value.evidence_count
    );
    Ok(CommandOutput::new(to_value(&value)?, human))
}

fn roots(path: &Path, args: &PagedAtArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let graph = GraphReader::new(&connection).paged_at(args.at, args.cursor.as_deref())?;
    let requested = graph.revision();
    let roots = graph.roots_page(cli_page_limit(args.limit)?, args.cursor.as_deref())?;
    let human = render_summary_page("Roots", &roots, requested);
    Ok(CommandOutput::new(
        json!({ "revision": requested, "roots": roots }),
        human,
    ))
}

fn show_concept(path: &Path, args: &ConceptShowArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let reader = GraphReader::new(&connection);
    let graph = args
        .at
        .map_or_else(|| reader.head(), |revision| reader.at(revision))?;
    let requested = graph.revision();
    let detail = graph.concept_detail(args.id, args.preview_limit)?;
    let evidence_preview = if detail.evidence.items.is_empty() {
        "  None".to_owned()
    } else {
        detail
            .evidence
            .items
            .iter()
            .map(|item| {
                format!(
                    "  {} — {}",
                    render_quoted(&item.work),
                    render_quoted(&item.quote)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let human = format!(
        "{} ({}) @ r{}\n{} parents · {} children · {} evidence links\n\nParents preview ({} of {})\n{}\n\nChildren preview ({} of {})\n{}\n\nEvidence preview ({} of {})\n{}",
        render_terminal_text(&detail.summary.label, false),
        detail.summary.id,
        requested,
        detail.summary.parent_count,
        detail.summary.child_count,
        detail.summary.evidence_count,
        detail.parents.page.returned,
        detail.parents.page.total,
        render_reference_page(&detail.parents),
        detail.children.page.returned,
        detail.children.page.total,
        render_reference_page(&detail.children),
        detail.evidence.page.returned,
        detail.evidence.page.total,
        evidence_preview
    );
    Ok(CommandOutput::new(
        json!({ "revision": requested, "concept": detail }),
        human,
    ))
}

fn concept_neighbors(
    path: &Path,
    args: &ConceptPageArgs,
    direction: GraphDirection,
) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let graph =
        GraphReader::new(&connection).paged_at(args.page.at, args.page.cursor.as_deref())?;
    let requested = graph.revision();
    let neighbor_direction = match direction {
        GraphDirection::Parents => NeighborDirection::Parents,
        GraphDirection::Children => NeighborDirection::Children,
        GraphDirection::Both => {
            return Err(AppError::invalid(
                "invalid_direction",
                "concept neighbors must be parents or children",
            ));
        }
    };
    let reference = graph.reference(args.id)?;
    let page = graph.neighbor_page(
        args.id,
        neighbor_direction,
        cli_page_limit(args.page.limit)?,
        args.page.cursor.as_deref(),
    )?;
    let heading = match direction {
        GraphDirection::Parents => "Parents",
        GraphDirection::Children => "Children",
        GraphDirection::Both => {
            return Err(AppError::invalid(
                "invalid_direction",
                "concept neighbors must be parents or children",
            ));
        }
    };
    let human = render_reference_page_heading(heading, &reference, &page, requested);
    let data = match direction {
        GraphDirection::Parents => {
            json!({ "revision": requested, "concept": reference, "parents": page })
        }
        GraphDirection::Children => {
            json!({ "revision": requested, "concept": reference, "children": page })
        }
        GraphDirection::Both => unreachable!("both was rejected above"),
    };
    Ok(CommandOutput::new(data, human))
}

fn concept_evidence(path: &Path, args: &ConceptPageArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let graph =
        GraphReader::new(&connection).paged_at(args.page.at, args.page.cursor.as_deref())?;
    let requested = graph.revision();
    let reference = graph.reference(args.id)?;
    let evidence = graph.evidence_page(
        args.id,
        cli_page_limit(args.page.limit)?,
        args.page.cursor.as_deref(),
    )?;
    let body = if evidence.items.is_empty() {
        "  None".to_owned()
    } else {
        evidence
            .items
            .iter()
            .map(|item| {
                format!(
                    "  {} — {}",
                    render_quoted(&item.work),
                    render_quoted(&item.quote)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let human = format!(
        "Evidence for {} at revision {requested} — showing {} of {}\n{body}{}",
        render_reference(&reference),
        evidence.page.returned,
        evidence.page.total,
        render_continuation(&evidence.page, requested)
    );
    Ok(CommandOutput::new(
        json!({ "revision": requested, "concept": reference, "evidence": evidence }),
        human,
    ))
}

fn graph(path: &Path, args: &GraphArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let reader = GraphReader::new(&connection);
    let graph = args
        .at
        .map_or_else(|| reader.head(), |revision| reader.at(revision))?;
    let direction = match args.direction {
        CliGraphDirection::Parents => GraphDirection::Parents,
        CliGraphDirection::Children => GraphDirection::Children,
        CliGraphDirection::Both => GraphDirection::Both,
    };
    let output = graph.graph_view(args.id, direction, args.depth, args.max_nodes)?;
    let human = render_graph(&output);
    Ok(CommandOutput::new(to_value(&output)?, human))
}

fn search(path: &Path, args: &SearchArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let graph = GraphReader::new(&connection).paged_at(args.at, args.cursor.as_deref())?;
    let requested = graph.revision();
    let output = graph.search(
        &args.query,
        args.within,
        cli_page_limit(args.limit)?,
        args.cursor.as_deref(),
    )?;
    let body = if output.results.items.is_empty() {
        "  None".to_owned()
    } else {
        output
            .results
            .items
            .iter()
            .enumerate()
            .map(|(index, result)| {
                format!(
                    "  {}. {} ({}) — {} parent(s), {} child(ren), {} evidence link(s){}",
                    index + 1,
                    render_terminal_text(&result.concept.label, false),
                    result.concept.id,
                    result.concept.parent_count,
                    result.concept.child_count,
                    result.concept.evidence_count,
                    if result.concept.shared {
                        " [shared]"
                    } else {
                        ""
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let human = format!(
        "Search at revision {requested} — showing {} of {}\n{body}{}",
        output.results.page.returned,
        output.results.page.total,
        render_continuation(&output.results.page, requested)
    );
    Ok(CommandOutput::new(to_value(&output)?, human))
}

fn cli_page_limit(limit: usize) -> Result<usize, AppError> {
    if (1..=200).contains(&limit) {
        Ok(limit)
    } else {
        Err(AppError::invalid(
            "invalid_limit",
            "a CLI page limit must be between 1 and 200",
        ))
    }
}

fn render_summary_page(heading: &str, page: &Page<ConceptSummary>, revision: i64) -> String {
    let body = if page.items.is_empty() {
        "  None".to_owned()
    } else {
        page.items
            .iter()
            .map(|item| {
                format!(
                    "  {} ({}){}",
                    render_terminal_text(&item.label, false),
                    item.id,
                    if item.shared {
                        format!(" [shared: {} parents]", item.parent_count)
                    } else {
                        String::new()
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "{heading} at revision {revision} — showing {} of {}\n{body}{}",
        page.page.returned,
        page.page.total,
        render_continuation(&page.page, revision)
    )
}

fn render_reference_page(page: &Page<ConceptReference>) -> String {
    if page.items.is_empty() {
        "  None".to_owned()
    } else {
        page.items
            .iter()
            .map(|item| format!("  {}", render_reference(item)))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn render_reference_page_heading(
    heading: &str,
    concept: &ConceptReference,
    page: &Page<ConceptReference>,
    revision: i64,
) -> String {
    format!(
        "{heading} of {} at revision {revision} — showing {} of {}\n{}{}",
        render_reference(concept),
        page.page.returned,
        page.page.total,
        render_reference_page(page),
        render_continuation(&page.page, revision)
    )
}

fn render_continuation(page: &PageInfo, revision: i64) -> String {
    page.next_cursor
        .as_ref()
        .map_or_else(String::new, |cursor| {
            format!("\nMore at revision {revision}: rerun with --at {revision} --cursor {cursor}")
        })
}

fn render_graph(graph: &GraphView) -> String {
    let nodes = graph
        .nodes
        .iter()
        .map(|node| {
            format!(
                "  {} — {} — distance {}",
                node.summary.id,
                render_quoted(&node.summary.label),
                node.distance
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let edges = if graph.edges.is_empty() {
        "  None".to_owned()
    } else {
        graph
            .edges
            .iter()
            .map(|edge| format!("  {} -> {}", edge.parent_id, edge.child_id))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let frontier = if graph.frontier.is_empty() {
        "  None".to_owned()
    } else {
        graph
            .frontier
            .iter()
            .map(|entry| {
                format!(
                    "  {} — {} unreturned parent(s), {} unreturned child(ren)",
                    entry.id, entry.unreturned_parent_count, entry.unreturned_child_count
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let coverage = if graph.node_limit_reached {
        format!(
            "Node limit reached at {}; expand again from a frontier concept",
            graph.max_nodes
        )
    } else {
        format!("Complete through requested depth {}", graph.depth)
    };
    let direction = match graph.direction {
        GraphDirection::Parents => "parents",
        GraphDirection::Children => "children",
        GraphDirection::Both => "both",
    };
    format!(
        "Graph around {} at revision {}\nDirection: {} · depth: {} · max nodes: {}\n\nNodes ({})\n{}\n\nEdges ({})\n{edges}\n\nFrontier\n{frontier}\n\n{coverage}",
        graph.seed,
        graph.revision,
        direction,
        graph.depth,
        graph.max_nodes,
        graph.nodes.len(),
        nodes,
        graph.edges.len()
    )
}

fn shake(path: &Path, args: &ShakeArgs, json_output: bool) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let plan = plan_shake(&connection)?;
    drop(connection);
    let report = render_shake_plan(&plan);
    if plan.removed_edges.is_empty() {
        return Ok(CommandOutput::new(
            shake_data(&plan, "unchanged", plan.base_revision),
            format!(
                "{report}\n\nThe graph is already transitively reduced; corpus remains at revision {}",
                plan.base_revision
            ),
        ));
    }
    if json_output && !args.yes {
        return Ok(CommandOutput::new(
            shake_data(&plan, "confirmation_required", plan.base_revision),
            "",
        ));
    }
    if !args.yes {
        println!("{report}\n");
        io::stdout().flush().map_err(|error| {
            AppError::unexpected(
                "report_write_failed",
                format!("unable to write shake report: {error}"),
            )
        })?;
        eprint!(
            "Remove {} transitively implied parent {}? [y/N] ",
            plan.removed_edges.len(),
            if plan.removed_edges.len() == 1 {
                "edge"
            } else {
                "edges"
            }
        );
        io::stderr().flush().map_err(|error| {
            AppError::unexpected(
                "confirmation_write_failed",
                format!("unable to write shake confirmation: {error}"),
            )
        })?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer).map_err(|error| {
            AppError::unexpected(
                "confirmation_read_failed",
                format!("unable to read shake confirmation: {error}"),
            )
        })?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            return Ok(CommandOutput::new(
                shake_data(&plan, "cancelled", plan.base_revision),
                format!(
                    "Shake cancelled; corpus remains at revision {}",
                    plan.base_revision
                ),
            ));
        }
    }
    let mut connection = db::open_write(path)?;
    let new_revision = apply_shake(&mut connection, &plan)?;
    let human = if args.yes {
        format!("{report}\n\nApplied shake as revision {new_revision}")
    } else {
        format!("Applied shake as revision {new_revision}")
    };
    Ok(CommandOutput::new(shake_data(&plan, "applied", new_revision), human).mutation())
}

fn render_shake_plan(plan: &ShakePlan) -> String {
    let edges = plan
        .removed_edges
        .iter()
        .map(|edge| {
            format!(
                "  {} → {}",
                render_reference(&edge.parent),
                render_reference(&edge.child)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let edge_list = if edges.is_empty() {
        String::new()
    } else {
        format!("\n\nEdges to remove:\n{edges}")
    };
    format!(
        "Shake revision {}\n\n{} of {} parent edges will be removed.\n{} parent edges will remain.\nConcepts, evidence, and ancestor/descendant reachability will remain unchanged.{edge_list}",
        plan.base_revision,
        plan.removed_edges.len(),
        plan.edge_count_before,
        plan.edge_count_after
    )
}

fn shake_data(plan: &ShakePlan, status: &str, revision: i64) -> Value {
    json!({
        "status": status,
        "base_revision": plan.base_revision,
        "revision": revision,
        "edge_count_before": plan.edge_count_before,
        "removed_edge_count": plan.removed_edges.len(),
        "edge_count_after": plan.edge_count_after,
        "removed_edges": plan.removed_edges
    })
}

fn lately(path: &Path, args: &LatelyArgs) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let report = ingestion::lately(&connection, args)?;
    let mut lines = vec![
        "Source activity".to_owned(),
        format!(
            "Range: {} to {} UTC (end exclusive)",
            report.since, report.until
        ),
        format!("Time basis: {}", args.by.display()),
    ];
    if let Some(status) = args.status {
        lines.push(format!("Status filter: {}", status.as_str()));
    }
    if let Some(channel) = args.channel {
        lines.push(format!("Channel filter: {}", channel.as_str()));
    }
    if report.delivery_count == 0 {
        lines.push("No source activity".to_owned());
    } else {
        lines.push(format!(
            "{} {}: {} completed, {} processing, {} failed; {} {}, {} {}",
            report.delivery_count,
            if report.delivery_count == 1 {
                "delivery"
            } else {
                "deliveries"
            },
            report.completed_count,
            report.processing_count,
            report.failed_count,
            report.new_work_count,
            if report.new_work_count == 1 {
                "new work"
            } else {
                "new works"
            },
            report.duplicate_count,
            if report.duplicate_count == 1 {
                "duplicate"
            } else {
                "duplicates"
            },
        ));
        lines.push(String::new());
        for delivery in &report.deliveries {
            let timestamp = delivery.selected_timestamp(args.by).ok_or_else(|| {
                AppError::database(
                    "missing_selected_timestamp",
                    "a selected source delivery has no timestamp for the report basis",
                )
            })?;
            let outcome = delivery.result.as_deref().map_or_else(
                || {
                    delivery.error.as_ref().map_or_else(
                        || delivery.status.clone(),
                        |error| {
                            if delivery.status == "failed" {
                                format!("failed ({})", error.code)
                            } else {
                                format!("retryable error ({})", error.code)
                            }
                        },
                    )
                },
                |result| {
                    if let Some(revision) = delivery.applied_revision {
                        format!("{result} r{revision}")
                    } else {
                        result.to_owned()
                    }
                },
            );
            let retention = delivery
                .retention
                .as_deref()
                .map_or_else(String::new, |value| format!("; {value} work"));
            lines.push(format!(
                "{}\t{}\t{}\t{}\t{}{}",
                render_terminal_text(timestamp, false),
                delivery.channel,
                delivery.status,
                render_terminal_text(&delivery.source_name, false),
                outcome,
                retention,
            ));
        }
    }
    if report.missing_time_count > 0 {
        lines.push(String::new());
        lines.push(format!(
            "{} deliveries omitted because {} time is unavailable",
            report.missing_time_count,
            args.by.as_str().replace('-', " ")
        ));
    }
    Ok(CommandOutput::new(to_value(&report)?, lines.join("\n")))
}

fn log(path: &Path, limit: usize) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let commits = list_commits(&connection, limit)?;
    let head = revision(&connection)?;
    let human = if commits.is_empty() {
        "No corpus commits".to_owned()
    } else {
        commits
            .iter()
            .map(|commit| {
                format!(
                    "r{}\t{}\t{}",
                    commit.revision,
                    commit.kind,
                    render_terminal_text(&commit.summary, false)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(
        json!({ "head_revision": head, "commits": commits }),
        human,
    ))
}

fn diff_revisions(path: &Path, from: i64, to: i64) -> Result<CommandOutput, AppError> {
    let connection = db::open_read(path)?;
    let value = diff(&connection, from, to)?;
    let human = if value.entries.is_empty() {
        format!("No corpus difference between revisions {from} and {to}")
    } else {
        value
            .entries
            .iter()
            .map(render_diff_entry)
            .collect::<Vec<_>>()
            .join("\n")
    };
    Ok(CommandOutput::new(to_value(&value)?, human))
}

fn render_diff_entry(entry: &DiffEntry) -> String {
    match entry {
        DiffEntry::Created { concept } => {
            format!("Created: {}", render_reference(concept))
        }
        DiffEntry::Retired { concept } => {
            format!("Retired: {}", render_reference(concept))
        }
        DiffEntry::Reworded { id, before, after } => format!(
            "Reworded {id}: {} → {}",
            render_quoted(before),
            render_quoted(after)
        ),
        DiffEntry::ParentAdded { parent, child } => format!(
            "Parent added: {} → {}",
            render_reference(parent),
            render_reference(child)
        ),
        DiffEntry::ParentRemoved { parent, child } => format!(
            "Parent removed: {} → {}",
            render_reference(parent),
            render_reference(child)
        ),
        DiffEntry::EvidenceAdded {
            concept,
            work,
            quote,
        } => format!(
            "Evidence added: {} — {} — {}",
            render_reference(concept),
            render_quoted(work),
            render_quoted(quote)
        ),
        DiffEntry::EvidenceRemoved {
            concept,
            work,
            quote,
        } => format!(
            "Evidence removed: {} — {} — {}",
            render_reference(concept),
            render_quoted(work),
            render_quoted(quote)
        ),
    }
}

fn revert(path: &Path, target: i64) -> Result<CommandOutput, AppError> {
    let mut connection = db::open_write(path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let new_revision = crate::corpus::revert(&transaction, target)?;
    transaction.commit()?;
    Ok(CommandOutput::new(
        json!({
            "revision": new_revision,
            "reverted_revision": target,
            "summary": format!("Revert revision {target}")
        }),
        format!("Applied revision {new_revision}:\nRevert revision {target}"),
    )
    .mutation())
}

pub(crate) fn read_utf8(path: &Path, description: &str) -> Result<String, AppError> {
    let bytes = if path == Path::new("-") {
        let mut bytes = Vec::new();
        io::stdin().read_to_end(&mut bytes).map_err(|error| {
            AppError::unexpected(
                "input_read_failed",
                format!("unable to read {description} from standard input: {error}"),
            )
        })?;
        bytes
    } else {
        fs::read(path).map_err(|error| {
            AppError::unexpected(
                "input_read_failed",
                format!("unable to read {description} {}: {error}", path.display()),
            )
        })?
    };
    String::from_utf8(bytes).map_err(|_| {
        AppError::invalid(
            "input_not_utf8",
            format!("{description} must be valid UTF-8"),
        )
    })
}

pub(crate) fn work_label(path: &Path, explicit: Option<&str>) -> Result<String, AppError> {
    if let Some(label) = explicit {
        return Ok(label.to_owned());
    }
    if path == Path::new("-") {
        return Err(AppError::invalid(
            "work_name_required",
            "a work read from standard input requires --name",
        ));
    }
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            AppError::invalid(
                "work_name_required",
                "the work path has no usable UTF-8 filename; supply --name",
            )
        })
}

fn count(connection: &Connection, table: &str) -> Result<u64, AppError> {
    count_where(connection, table, "1")
}

fn count_where(connection: &Connection, table: &str, condition: &str) -> Result<u64, AppError> {
    let count = connection.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE {condition}"),
        [],
        |row| row.get::<_, i64>(0),
    )?;
    u64::try_from(count)
        .map_err(|_| AppError::database("invalid_count", "database returned a negative count"))
}

fn stats_overflow() -> AppError {
    AppError::database("invalid_count", "corpus state is too large to report")
}

fn to_value<T: Serialize>(value: &T) -> Result<Value, AppError> {
    serde_json::to_value(value).map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::resolve_library_path;

    #[test]
    fn library_path_honors_precedence_and_ignores_an_empty_environment()
    -> Result<(), Box<dyn std::error::Error>> {
        let explicit = Path::new("explicit.db");
        let configured = Path::new("configured.db");

        assert_eq!(
            resolve_library_path(
                Some(explicit),
                Some(OsStr::new("environment.db")),
                Some(configured),
            )?,
            explicit
        );
        assert_eq!(
            resolve_library_path(None, Some(OsStr::new("environment.db")), Some(configured))?,
            Path::new("environment.db")
        );
        assert_eq!(
            resolve_library_path(None, Some(OsStr::new("")), Some(configured))?,
            configured
        );

        let error = resolve_library_path(None, Some(OsStr::new("")), None)
            .err()
            .ok_or("library resolution unexpectedly succeeded")?;
        assert_eq!(error.code(), "library_not_configured");
        Ok(())
    }
}
