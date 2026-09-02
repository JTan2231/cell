use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{Read as _, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};
use time::OffsetDateTime;

use crate::cli::{
    AgreementCommand, AttemptCommand, Cli, Command, ConformanceCommand, IntegrationCommand,
    NegotiationCommand, StewardCommand, TrackCommand,
};
use crate::error::{AppError, AppResult, Context as _};
use crate::model::{
    AgentAttemptView, BasisGuard, BasisKind, FrozenBasisView, NegotiationEventKind, NewFrozenBasis,
    NewFrozenSource, NewStewardScope, OpaqueMarkdown, PartyRole, sha256_hex,
};
use crate::nucleus::{AgentRunOutcome, AgentRunner};
use crate::source_catalog::FrozenSourceCatalog;
use crate::store::Store;

const MAX_FILE_BYTES: u64 = 1024 * 1024;

pub(crate) enum HumanOutput {
    Text(String),
    Exact(Vec<u8>),
}

pub(crate) struct CommandOutput {
    pub(crate) data: Value,
    pub(crate) human: HumanOutput,
}

impl CommandOutput {
    fn value(data: Value) -> AppResult<Self> {
        let human = serde_json::to_string_pretty(&data).context(
            "json_serialization_failed",
            "unable to render command output",
        )?;
        Ok(Self {
            data,
            human: HumanOutput::Text(human),
        })
    }

    fn serializable<T: Serialize>(kind: &str, value: &T) -> AppResult<Self> {
        Self::value(json!({"type": kind, "value": value}))
    }
}

fn agent_run_output(
    kind: &str,
    store: &Store,
    result: &AgentRunOutcome,
) -> AppResult<CommandOutput> {
    CommandOutput::value(json!({
        "type": kind,
        "value": {
            "attempt": attempt_projection(store, &result.attempt)?,
            "result_kind": result.result_kind,
            "result_id": result.result_id,
        }
    }))
}

fn attempt_projection(store: &Store, attempt: &AgentAttemptView) -> AppResult<Value> {
    let sources = attempt
        .sources
        .iter()
        .map(|source| {
            json!({
                "source_id": source.source_id,
                "kind": source.kind,
                "locator": source.locator,
                "origin_path": source.origin_path,
                "revision": source.revision,
                "content_sha256": source.content_sha256,
                "content_bytes": source.content.len(),
                "observed_at": source.observed_at,
            })
        })
        .collect::<Vec<_>>();
    let receipts = store
        .attempt_receipts(&attempt.attempt_id)?
        .into_iter()
        .map(|receipt| {
            json!({
                "receipt_id": receipt.receipt_id,
                "call_id": receipt.call_id,
                "arguments_sha256": receipt.arguments_sha256,
                "result_sha256": sha256_hex(&receipt.result_json),
                "result_bytes": receipt.result_json.len(),
                "is_error": receipt.is_error,
                "domain_result_kind": receipt.domain_result_kind,
                "domain_result_id": receipt.domain_result_id,
                "source_refs": receipt.emitted_source_refs,
                "recorded_at": receipt.recorded_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "attempt_id": attempt.attempt_id,
        "predecessor_attempt_id": attempt.predecessor_attempt_id,
        "kind": attempt.kind,
        "subject_id": attempt.subject_id,
        "requester_id": attempt.requester_id,
        "nucleus_job_id": attempt.nucleus_job_id,
        "request_sha256": attempt.request_sha256,
        "request_bytes": attempt.request_bytes.len(),
        "toolset": {
            "provider": "pratica",
            "name": attempt.toolset_name,
            "version": attempt.toolset_version,
        },
        "expected_offer_id": attempt.expected_offer_id,
        "expected_roster_digest": attempt.expected_roster_digest,
        "basis_id": attempt.basis_id,
        "basis_digest": attempt.basis_digest,
        "catalog": {
            "scope": attempt.catalog_scope,
            "version": attempt.catalog_version,
            "verifier_version": attempt.catalog_verifier_version,
            "observed_at": attempt.catalog_observed_at,
            "party": attempt.catalog_party,
            "title": attempt.catalog_title,
            "charter_sha256": attempt.catalog_charter_sha256,
            "catalog_sha256": attempt.catalog_sha256,
        },
        "sources": sources,
        "tool_after": attempt.tool_after,
        "admitted": attempt.admitted,
        "accepted_job_id": attempt.accepted_job_id,
        "accepted_request_sha256": attempt.accepted_request_sha256,
        "active": attempt.active,
        "runtime_state": attempt.runtime_state,
        "runtime_detail": attempt.runtime_detail,
        "domain_result_kind": attempt.domain_result_kind,
        "domain_result_id": attempt.domain_result_id,
        "receipts": receipts,
        "created_at": attempt.created_at,
        "updated_at": attempt.updated_at,
    }))
}

#[allow(clippy::too_many_lines)]
pub(crate) fn run(cli: &Cli) -> AppResult<CommandOutput> {
    let database = resolve_database(cli.database.as_deref())?;
    match &cli.command {
        Command::Init => {
            let result = Store::init(&database)?;
            CommandOutput::serializable("initialized", &result)
        }
        Command::Doctor => {
            let storage = Store::doctor(&database)?;
            AgentRunner::for_current_user().doctor()?;
            CommandOutput::value(json!({
                "type": "doctor",
                "database": database,
                "storage": storage,
                "nucleus": "ready",
                "toolsets": [
                    "pratica/steward-response/1",
                    "pratica/composition-review/1",
                    "pratica/conformance-review/1"
                ]
            }))
        }
        Command::Steward { command } => steward_command(&database, command),
        Command::Integration { command } => integration_command(&database, command),
        Command::Track { command } => track_command(&database, command),
        Command::Negotiation { command } => negotiation_command(&database, command),
        Command::Attempt { command } => attempt_command(&database, command),
        Command::Agreement { command } => agreement_command(&database, command),
        Command::Conformance { command } => conformance_command(&database, command),
    }
}

fn steward_command(database: &Path, command: &StewardCommand) -> AppResult<CommandOutput> {
    match command {
        StewardCommand::Register { manifest } => {
            let catalog = FrozenSourceCatalog::load(manifest)?;
            let (scope, basis) = steward_inputs(&catalog)?;
            let mut store = Store::open_write(database)?;
            let (scope, basis) = store.register_steward(&scope, &basis)?;
            CommandOutput::value(json!({
                "type": "steward_registered",
                "scope": scope,
                "basis": basis,
            }))
        }
        StewardCommand::List => {
            let store = Store::open_read(database)?;
            CommandOutput::serializable("steward_list", &store.list_steward_scopes()?)
        }
        StewardCommand::Show { scope, version } => {
            let store = Store::open_read(database)?;
            let version = select_scope_version(&store, scope, *version)?;
            CommandOutput::serializable("steward_scope", &store.steward_scope(scope, version)?)
        }
        StewardCommand::Respond { negotiation } => {
            let mut store = Store::open_write(database)?;
            let result =
                AgentRunner::for_current_user().steward_response(&mut store, negotiation)?;
            agent_run_output("steward_attempt", &store, &result)
        }
    }
}

fn integration_command(database: &Path, command: &IntegrationCommand) -> AppResult<CommandOutput> {
    match command {
        IntegrationCommand::Open(arguments) => {
            let context = arguments
                .context
                .as_deref()
                .map(read_markdown)
                .transpose()?;
            let mut store = Store::open_write(database)?;
            let integration =
                store.create_integration(&arguments.entrant, &arguments.title, context.as_ref())?;
            CommandOutput::serializable("integration_opened", &integration)
        }
        IntegrationCommand::Status(arguments) => {
            let store = Store::open_read(database)?;
            CommandOutput::serializable(
                "integration_status",
                &store.integration_status(&arguments.id)?,
            )
        }
        IntegrationCommand::Review(arguments) => {
            let mut store = Store::open_write(database)?;
            let result =
                AgentRunner::for_current_user().composition_review(&mut store, &arguments.id)?;
            agent_run_output("composition_attempt", &store, &result)
        }
        IntegrationCommand::Report(arguments) => {
            let store = Store::open_read(database)?;
            let status = store.integration_status(&arguments.id)?;
            let data = serde_json::to_value(&status).context(
                "json_serialization_failed",
                "unable to encode integration report",
            )?;
            let human = render_integration_report(&status);
            Ok(CommandOutput {
                data: json!({"type": "integration_report", "value": data}),
                human: HumanOutput::Text(human),
            })
        }
    }
}

fn track_command(database: &Path, command: &TrackCommand) -> AppResult<CommandOutput> {
    match command {
        TrackCommand::Open(arguments) => {
            let terms = read_markdown(&arguments.terms)?;
            let mut store = Store::open_write(database)?;
            let version =
                select_scope_version(&store, &arguments.steward, arguments.steward_version)?;
            let (track, negotiation) =
                store.open_track(&arguments.integration, &arguments.steward, version, &terms)?;
            CommandOutput::value(json!({
                "type": "track_opened",
                "track": track,
                "negotiation": negotiation,
            }))
        }
        TrackCommand::Retire { track, reason } => {
            let mut store = Store::open_write(database)?;
            CommandOutput::serializable("track_retired", &store.retire_track(track, reason)?)
        }
    }
}

fn negotiation_command(database: &Path, command: &NegotiationCommand) -> AppResult<CommandOutput> {
    match command {
        NegotiationCommand::Show(arguments) => {
            let store = Store::open_read(database)?;
            CommandOutput::serializable("negotiation", &store.negotiation(&arguments.id)?)
        }
        NegotiationCommand::History(arguments) => {
            let store = Store::open_read(database)?;
            CommandOutput::serializable(
                "negotiation_history",
                &store.negotiation_history(&arguments.id)?,
            )
        }
        NegotiationCommand::Propose {
            negotiation,
            base,
            terms,
        } => {
            let terms = read_markdown(terms)?;
            let mut store = Store::open_write(database)?;
            CommandOutput::serializable(
                "entrant_proposal",
                &store.propose_as_entrant(negotiation, base, &terms)?,
            )
        }
        NegotiationCommand::Assent { negotiation, offer } => {
            let mut store = Store::open_write(database)?;
            let guard = current_negotiation_guard(&mut store, negotiation)?;
            CommandOutput::serializable(
                "entrant_assent",
                &store.assent_as_entrant(negotiation, offer, Some(&guard))?,
            )
        }
        NegotiationCommand::Withdraw { negotiation, offer } => {
            let mut store = Store::open_write(database)?;
            CommandOutput::serializable(
                "entrant_assent_withdrawn",
                &store.withdraw_entrant_assent(negotiation, offer)?,
            )
        }
        NegotiationCommand::Cancel {
            negotiation,
            reason,
        } => {
            let mut store = Store::open_write(database)?;
            CommandOutput::serializable(
                "negotiation_cancelled",
                &store.cancel_negotiation(negotiation, reason)?,
            )
        }
    }
}

fn attempt_command(database: &Path, command: &AttemptCommand) -> AppResult<CommandOutput> {
    match command {
        AttemptCommand::Show(arguments) => {
            let store = Store::open_read(database)?;
            let attempt = store.attempt(&arguments.id)?;
            CommandOutput::value(json!({
                "type": "agent_attempt",
                "value": attempt_projection(&store, &attempt)?,
            }))
        }
        AttemptCommand::Retry(arguments) => {
            let mut store = Store::open_write(database)?;
            let result = AgentRunner::for_current_user().retry(&mut store, &arguments.id)?;
            agent_run_output("agent_attempt_retry", &store, &result)
        }
    }
}

fn agreement_command(database: &Path, command: &AgreementCommand) -> AppResult<CommandOutput> {
    match command {
        AgreementCommand::Show(arguments) => {
            let store = Store::open_read(database)?;
            CommandOutput::serializable("agreement", &store.agreement(&arguments.id)?)
        }
        AgreementCommand::Export { agreement, output } => {
            let store = Store::open_read(database)?;
            let agreement = store.agreement(agreement)?;
            let terms = agreement.offer.terms_markdown.as_bytes();
            if let Some(path) = output {
                write_absent_private_file(path, terms)?;
                return CommandOutput::value(json!({
                    "type": "agreement_exported",
                    "agreement_id": agreement.agreement_id,
                    "offer_id": agreement.offer.offer_id,
                    "terms_sha256": agreement.offer.terms_sha256,
                    "output": path,
                    "bytes": terms.len(),
                }));
            }
            let data = json!({
                "type": "agreement_export",
                "agreement_id": agreement.agreement_id,
                "offer_id": agreement.offer.offer_id,
                "terms_sha256": agreement.offer.terms_sha256,
                "terms_markdown": agreement.offer.terms_markdown,
            });
            Ok(CommandOutput {
                data,
                human: HumanOutput::Exact(terms.to_vec()),
            })
        }
        AgreementCommand::Verify(arguments) => {
            let mut store = Store::open_write(database)?;
            let agreement = store.checked_agreement(&arguments.id)?;
            let basis = store.frozen_basis(&agreement.basis_id)?;
            let observation = observe_basis(&basis)?;
            let agreement = store.verify_agreement(
                &arguments.id,
                observation.digest.as_deref(),
                observation.detail.as_ref(),
            )?;
            CommandOutput::serializable("agreement_verification", &agreement)
        }
        AgreementCommand::Amend { agreement, terms } => {
            let terms = read_markdown(terms)?;
            let mut store = Store::open_write(database)?;
            CommandOutput::serializable(
                "agreement_amendment_opened",
                &store.open_amendment(agreement, &terms)?,
            )
        }
    }
}

fn conformance_command(database: &Path, command: &ConformanceCommand) -> AppResult<CommandOutput> {
    match command {
        ConformanceCommand::Review {
            agreement,
            candidate_basis,
        } => {
            let candidate = FrozenSourceCatalog::load(candidate_basis)?;
            let mut store = Store::open_write(database)?;
            let result = AgentRunner::for_current_user()
                .conformance_review(&mut store, agreement, &candidate)?;
            agent_run_output("conformance_attempt", &store, &result)
        }
        ConformanceCommand::Show(arguments) => {
            let store = Store::open_read(database)?;
            CommandOutput::serializable(
                "conformance_review",
                &store.conformance_review(&arguments.id)?,
            )
        }
    }
}

pub(crate) fn resolve_database(explicit: Option<&Path>) -> AppResult<PathBuf> {
    let path = if let Some(path) = explicit {
        path.to_path_buf()
    } else if let Some(path) = env::var_os("PRATICA_DATABASE").filter(|value| !value.is_empty()) {
        PathBuf::from(path)
    } else {
        let home = env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::new(
                    "home_unavailable",
                    "HOME is required when --database and PRATICA_DATABASE are not set",
                )
            })?;
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Pratica")
            .join("pratica.db")
    };
    if !path.is_absolute() {
        return Err(AppError::usage(
            "database_path_relative",
            "Pratica database paths must be absolute",
        ));
    }
    Ok(path)
}

fn read_markdown(path: &Path) -> AppResult<OpaqueMarkdown> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .context(
            "markdown_read_failed",
            format!("unable to open {}", path.display()),
        )?;
    let metadata = file.metadata().context(
        "markdown_read_failed",
        format!("unable to inspect {}", path.display()),
    )?;
    if !metadata.is_file() {
        return Err(AppError::usage(
            "markdown_not_regular",
            format!(
                "Markdown input must be a nonsymlink regular file: {}",
                path.display()
            ),
        ));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(AppError::usage(
            "markdown_too_large",
            format!("Markdown input exceeds {MAX_FILE_BYTES} bytes"),
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .context(
            "markdown_read_failed",
            format!("unable to read {}", path.display()),
        )?;
    OpaqueMarkdown::new(bytes).map_err(Into::into)
}

fn steward_inputs(catalog: &FrozenSourceCatalog) -> AppResult<(NewStewardScope, NewFrozenBasis)> {
    let charter_markdown = OpaqueMarkdown::from_text(catalog.charter_markdown.clone())?;
    let scope = NewStewardScope {
        scope_id: catalog.scope.clone(),
        version: catalog.version,
        steward_party: catalog.party.clone(),
        title: catalog.title.clone(),
        charter_markdown,
        descriptor_sha256: catalog.catalog_sha256.clone(),
    };
    let basis = basis_from_catalog(catalog, BasisKind::Steward)?;
    Ok((scope, basis))
}

pub(crate) fn basis_from_catalog(
    catalog: &FrozenSourceCatalog,
    kind: BasisKind,
) -> AppResult<NewFrozenBasis> {
    let (scope_id, scope_version) = if kind == BasisKind::Steward {
        (Some(catalog.scope.clone()), Some(catalog.version))
    } else {
        (None, None)
    };
    let sources = catalog
        .sources
        .iter()
        .map(|source| {
            let origin_path = source.origin_path.to_str().ok_or_else(|| {
                AppError::usage(
                    "source_path_not_utf8",
                    "canonical source paths must be representable as UTF-8",
                )
            })?;
            Ok(NewFrozenSource {
                source_id: source.id.clone(),
                kind: source.kind.clone(),
                locator: source.locator.clone(),
                origin_path: Some(origin_path.to_owned()),
                revision: source.revision.clone(),
                content: source.content.clone(),
                observed_at: source.observed_at,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(NewFrozenBasis {
        kind,
        label: catalog.title.clone(),
        scope_id,
        scope_version,
        verifier_version: catalog.verifier_version.clone(),
        observed_at: catalog.observed_at,
        sources,
    })
}

fn select_scope_version(store: &Store, scope_id: &str, selected: Option<u32>) -> AppResult<u32> {
    if let Some(version) = selected {
        store.steward_scope(scope_id, version)?;
        return Ok(version);
    }
    store
        .list_steward_scopes()?
        .into_iter()
        .filter(|scope| scope.scope_id == scope_id)
        .map(|scope| scope.version)
        .max()
        .ok_or_else(|| {
            AppError::new(
                "not_found",
                format!("steward scope {scope_id} is not registered"),
            )
        })
}

struct BasisObservation {
    digest: Option<String>,
    detail: Option<OpaqueMarkdown>,
}

fn observe_basis(basis: &FrozenBasisView) -> AppResult<BasisObservation> {
    let mut sources = Vec::with_capacity(basis.sources.len());
    for source in &basis.sources {
        let Some(path) = source.origin_path.as_deref() else {
            return unknown_observation("a frozen source has no verifiable origin path");
        };
        let path = Path::new(path);
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                return unknown_observation(&format!(
                    "unable to inspect current source {}: {error}",
                    source.locator
                ));
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return stale_observation(
                basis,
                &format!("source identity changed: {}", source.locator),
            );
        }
        if metadata.len() > crate::model::MAX_FROZEN_SOURCE_BYTES as u64 {
            return stale_observation(
                basis,
                &format!("source now exceeds the admitted bound: {}", source.locator),
            );
        }
        let mut content = Vec::new();
        match fs::File::open(path).and_then(|file| {
            file.take(crate::model::MAX_FROZEN_SOURCE_BYTES as u64 + 1)
                .read_to_end(&mut content)
        }) {
            Ok(_) => {}
            Err(error) => {
                return unknown_observation(&format!(
                    "unable to read current source {}: {error}",
                    source.locator
                ));
            }
        }
        sources.push(NewFrozenSource {
            source_id: source.source_id.clone(),
            kind: source.kind.clone(),
            locator: source.locator.clone(),
            origin_path: source.origin_path.clone(),
            revision: source.revision.clone(),
            content,
            observed_at: OffsetDateTime::now_utc().unix_timestamp(),
        });
    }
    let current = NewFrozenBasis {
        kind: basis.kind,
        label: basis.label.clone(),
        scope_id: basis.scope_id.clone(),
        scope_version: basis.scope_version,
        verifier_version: basis.verifier_version.clone(),
        observed_at: OffsetDateTime::now_utc().unix_timestamp(),
        sources,
    };
    match Store::basis_manifest_sha256(&current) {
        Ok(digest) => Ok(BasisObservation {
            digest: Some(digest),
            detail: None,
        }),
        Err(error) => stale_observation(basis, &format!("current basis is inadmissible: {error}")),
    }
}

fn unknown_observation(detail: &str) -> AppResult<BasisObservation> {
    Ok(BasisObservation {
        digest: None,
        detail: Some(OpaqueMarkdown::from_text(detail)?),
    })
}

fn stale_observation(basis: &FrozenBasisView, detail: &str) -> AppResult<BasisObservation> {
    let digest = sha256_hex(
        format!(
            "pratica-stale-basis-v1\0{}\0{detail}",
            basis.manifest_sha256
        )
        .as_bytes(),
    );
    Ok(BasisObservation {
        digest: Some(digest),
        detail: Some(OpaqueMarkdown::from_text(detail)?),
    })
}

fn current_negotiation_guard(store: &mut Store, negotiation_id: &str) -> AppResult<BasisGuard> {
    let negotiation = store.negotiation(negotiation_id)?;
    let head = negotiation
        .head
        .as_ref()
        .ok_or_else(|| AppError::new("corrupt_state", "open negotiation has no head offer"))?;
    let basis_id = if let Some(basis_id) = &head.basis_id {
        basis_id.clone()
    } else {
        store
            .negotiation_history(negotiation_id)?
            .into_iter()
            .rev()
            .find(|event| {
                event.kind == NegotiationEventKind::Assent
                    && event.party_role == Some(PartyRole::Steward)
                    && event.offer_id.as_deref() == Some(head.offer_id.as_str())
            })
            .and_then(|event| event.basis_id)
            .ok_or_else(|| {
                AppError::new(
                    "steward_assent_missing",
                    "entrant cannot seal terms before the steward assents on a current basis",
                )
            })?
    };
    let basis = store.frozen_basis(&basis_id)?;
    let observation = observe_basis(&basis)?;
    store.record_basis_verification(
        &basis_id,
        observation.digest.as_deref(),
        observation.detail.as_ref(),
    )?;
    let digest = observation.digest.ok_or_else(|| {
        AppError::new(
            "basis_unavailable",
            "the steward basis could not be verified from its recorded sources",
        )
    })?;
    if digest != basis.manifest_sha256 {
        return Err(AppError::new(
            "basis_stale",
            "the steward basis changed after its response",
        ));
    }
    Ok(BasisGuard {
        basis_id,
        observed_manifest_sha256: digest,
    })
}

fn write_absent_private_file(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if !path.is_absolute() {
        return Err(AppError::usage(
            "export_path_relative",
            "agreement export paths must be absolute",
        ));
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .context(
            "export_failed",
            format!("unable to create {}", path.display()),
        )?;
    file.write_all(bytes).context(
        "export_failed",
        format!("unable to write {}", path.display()),
    )?;
    file.sync_all().context(
        "export_failed",
        format!("unable to sync {}", path.display()),
    )
}

fn render_integration_report(status: &crate::model::IntegrationStatusView) -> String {
    let mut report = format!(
        "# {}\n\nIntegration: `{}`\n\nReady: **{}**\n\nRoster revision: {}\n\n## Bilateral tracks\n",
        status.integration.title,
        status.integration.integration_id,
        status.ready,
        status.roster.revision,
    );
    for track in &status.tracks {
        let agreement = track.active_agreement.as_ref().map_or_else(
            || "unsealed".to_owned(),
            |agreement| {
                format!(
                    "{} (basis {}, {})",
                    agreement.agreement_id,
                    agreement.basis_id,
                    agreement.basis_freshness.as_str(),
                )
            },
        );
        let negotiation = track
            .negotiation
            .as_ref()
            .map_or("none".to_owned(), |value| {
                let head = value
                    .head
                    .as_ref()
                    .map_or("none", |offer| offer.offer_id.as_str());
                format!(
                    "{} ({}, head {}, entrant {}, steward {})",
                    value.negotiation_id,
                    value.status.as_str(),
                    head,
                    value.entrant.status.as_str(),
                    value.steward.status.as_str(),
                )
            });
        let _ = write!(
            report,
            "\n- `{}` — steward `{}` v{}; negotiation {}; agreement `{}`; renegotiating {}",
            track.track.track_id,
            track.track.scope_id,
            track.track.scope_version,
            negotiation,
            agreement,
            track.renegotiating,
        );
    }
    if let Some(review) = &status.latest_composition_review {
        let _ = write!(
            report,
            "\n\n## Composition review\n\n`{}` — outcome `{}`, stale {}\n\n{}\n",
            review.review_id,
            review.outcome.as_str(),
            review.stale,
            review.review_markdown.as_str(),
        );
    } else {
        report.push_str("\n\n## Composition review\n\nNo composition review has been recorded.\n");
    }
    report.push_str(
        "\n\n## Limits\n\nThis report covers only the registered active tracks. It does not prove exhaustive system discovery, implementation conformance, deployment readiness, or authority to change a target system.\n",
    );
    report
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt as _, symlink};
    use std::path::Path;

    use super::{read_markdown, resolve_database, write_absent_private_file};

    #[test]
    fn explicit_database_must_be_absolute() {
        assert!(resolve_database(Some(Path::new("relative.db"))).is_err());
        assert_eq!(
            resolve_database(Some(Path::new("/tmp/pratica-test.db"))).expect("absolute path"),
            Path::new("/tmp/pratica-test.db")
        );
    }

    #[test]
    fn exact_file_io_preserves_bytes_and_refuses_symlinks_or_overwrite()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let exact = "# Terms\r\n\r\nUnicode: π\r\n".as_bytes();
        let input = directory.path().join("terms.md");
        fs::write(&input, exact)?;
        assert_eq!(read_markdown(&input)?.as_bytes(), exact);

        let linked = directory.path().join("linked.md");
        symlink(&input, &linked)?;
        assert!(read_markdown(&linked).is_err());

        let output = directory.path().join("agreement.md");
        write_absent_private_file(&output, exact)?;
        assert_eq!(fs::read(&output)?, exact);
        assert_eq!(fs::metadata(&output)?.permissions().mode() & 0o777, 0o600);
        assert!(write_absent_private_file(&output, b"replacement").is_err());
        assert_eq!(fs::read(&output)?, exact);
        Ok(())
    }
}
