#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{
    Connection, OpenFlags, OptionalExtension as _, Transaction, TransactionBehavior, params,
};
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::model::{
    AgentAttemptView, AgreementView, AssentStatus, AttemptKind, AttemptSourceInput, BasisFreshness,
    BasisGuard, BasisKind, BasisVerificationView, CompositionAgreementRef, CompositionOutcome,
    CompositionReviewView, ConformanceOutcome, ConformanceReviewView, FrozenBasisView,
    FrozenSourceView, IntegrationStatusView, IntegrationView, MAX_AGENT_REQUEST_BYTES,
    MAX_FROZEN_CATALOG_BYTES, MAX_FROZEN_SOURCE_BYTES, MAX_FROZEN_SOURCES, MAX_TOOL_RESULT_BYTES,
    MutationResult, NegotiationEventKind, NegotiationEventView, NegotiationKind, NegotiationStatus,
    NegotiationView, NewAgentAttempt, NewFrozenBasis, NewFrozenSource, NewStewardScope, OfferView,
    OpaqueMarkdown, PartyAssentView, PartyRole, RosterView, RuntimeState, StewardResponse,
    StewardScopeView, ToolReceiptView, TrackStatusView, TrackView, sha256_hex,
};

const SCHEMA: &str = include_str!("../schema.sql");
pub const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StoreErrorKind {
    InvalidInput,
    NotFound,
    Conflict,
    Stale,
    Database,
    Filesystem,
    CorruptState,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Stale(String),
    #[error("database operation failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("filesystem operation failed: {0}")]
    Filesystem(#[from] std::io::Error),
    #[error("{0}")]
    CorruptState(String),
}

impl StoreError {
    pub const fn kind(&self) -> StoreErrorKind {
        match self {
            Self::InvalidInput(_) => StoreErrorKind::InvalidInput,
            Self::NotFound(_) => StoreErrorKind::NotFound,
            Self::Conflict(_) => StoreErrorKind::Conflict,
            Self::Stale(_) => StoreErrorKind::Stale,
            Self::Database(_) => StoreErrorKind::Database,
            Self::Filesystem(_) => StoreErrorKind::Filesystem,
            Self::CorruptState(_) => StoreErrorKind::CorruptState,
        }
    }

    pub fn message(&self) -> String {
        self.to_string()
    }
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InitResult {
    pub schema_version: i64,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DoctorResult {
    pub schema_version: i64,
    pub integrity: String,
    pub foreign_keys: String,
    pub schema_objects: String,
    pub digests: String,
    pub protocol_invariants: String,
    pub permissions: String,
}

pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn init(path: &Path) -> StoreResult<InitResult> {
        if path.exists() {
            return Err(StoreError::Conflict(format!(
                "database already exists at {}",
                path.display()
            )));
        }
        let parent = path.parent().ok_or_else(|| {
            StoreError::InvalidInput("database path must have a parent directory".into())
        })?;
        let parent_existed = parent.exists();
        fs::create_dir_all(parent)?;
        if parent_existed {
            require_private_directory(parent)?;
        } else {
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        configure_connection(&connection)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(SCHEMA)?;
        transaction.commit()?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        secure_sidecars(path)?;
        Ok(InitResult {
            schema_version: SCHEMA_VERSION,
            path: path.to_path_buf(),
        })
    }

    pub fn open_write(path: &Path) -> StoreResult<Self> {
        require_existing_private_file(path)?;
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        configure_connection(&connection)?;
        require_schema(&connection)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        secure_sidecars(path)?;
        Ok(Self { connection })
    }

    pub fn open_read(path: &Path) -> StoreResult<Self> {
        require_existing_private_file(path)?;
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        configure_connection(&connection)?;
        require_schema(&connection)?;
        Ok(Self { connection })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> StoreResult<Self> {
        let mut connection = Connection::open_in_memory()?;
        configure_connection(&connection)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(SCHEMA)?;
        transaction.commit()?;
        Ok(Self { connection })
    }

    pub fn doctor(path: &Path) -> StoreResult<DoctorResult> {
        let store = Self::open_read(path)?;
        let integrity: String =
            store
                .connection
                .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(StoreError::CorruptState(format!(
                "SQLite integrity_check returned {integrity}"
            )));
        }
        let violation: Option<String> = store
            .connection
            .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
            .optional()?;
        if let Some(table) = violation {
            return Err(StoreError::CorruptState(format!(
                "foreign-key violation in {table}"
            )));
        }
        require_schema_objects(&store.connection)?;
        verify_stored_digests(&store)?;
        verify_protocol_invariants(&store.connection)?;
        Ok(DoctorResult {
            schema_version: SCHEMA_VERSION,
            integrity,
            foreign_keys: "ok".into(),
            schema_objects: "ok".into(),
            digests: "ok".into(),
            protocol_invariants: "ok".into(),
            permissions: "private".into(),
        })
    }

    pub fn steward_scope(&self, scope_id: &str, version: u32) -> StoreResult<StewardScopeView> {
        read_scope(&self.connection, scope_id, version)
    }

    pub fn list_steward_scopes(&self) -> StoreResult<Vec<StewardScopeView>> {
        let mut statement = self.connection.prepare(
            "SELECT scope_id, version, steward_party, title, charter_markdown,
                    charter_sha256, descriptor_sha256, recorded_at
             FROM steward_scopes ORDER BY scope_id, version",
        )?;
        let rows = statement.query_map([], decode_scope_row)?;
        collect_rows(rows)
    }

    pub fn basis_manifest_sha256(input: &NewFrozenBasis) -> StoreResult<String> {
        validate_new_basis(input)?;
        let mut hasher = Sha256::new();
        put_digest_field(&mut hasher, b"pratica-frozen-basis-v1");
        put_digest_field(&mut hasher, input.kind.as_str().as_bytes());
        put_digest_field(&mut hasher, input.label.as_bytes());
        put_optional_digest_field(&mut hasher, input.scope_id.as_deref());
        hasher.update(input.scope_version.unwrap_or_default().to_be_bytes());
        put_digest_field(&mut hasher, input.verifier_version.as_bytes());
        hasher.update((input.sources.len() as u64).to_be_bytes());
        for source in &input.sources {
            validate_frozen_source(source)?;
            put_digest_field(&mut hasher, source.source_id.as_bytes());
            put_digest_field(&mut hasher, source.kind.as_bytes());
            put_digest_field(&mut hasher, source.locator.as_bytes());
            put_optional_digest_field(&mut hasher, source.revision.as_deref());
            put_digest_field(&mut hasher, sha256_hex(&source.content).as_bytes());
            hasher.update((source.content.len() as u64).to_be_bytes());
        }
        Ok(format!("{:x}", hasher.finalize()))
    }

    fn freeze_basis(&mut self, input: &NewFrozenBasis) -> StoreResult<FrozenBasisView> {
        let manifest_sha256 = Self::basis_manifest_sha256(input)?;
        let basis_id = new_id("bas");
        let now = now_unix();
        let transaction = self.immediate()?;
        if input.kind == BasisKind::Steward {
            let scope_id = input.scope_id.as_deref().ok_or_else(|| {
                StoreError::InvalidInput("steward basis requires a scope id".into())
            })?;
            let scope_version = input.scope_version.ok_or_else(|| {
                StoreError::InvalidInput("steward basis requires a scope version".into())
            })?;
            read_scope(&transaction, scope_id, scope_version)?;
        }
        transaction.execute(
            "INSERT INTO frozen_bases (
                basis_id, basis_kind, label, scope_id, scope_version,
                verifier_version, manifest_sha256, observed_at, recorded_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                basis_id,
                input.kind.as_str(),
                input.label,
                input.scope_id,
                input.scope_version,
                input.verifier_version,
                manifest_sha256,
                input.observed_at,
                now,
            ],
        )?;
        for (ordinal, source) in input.sources.iter().enumerate() {
            transaction.execute(
                "INSERT INTO frozen_basis_sources (
                    basis_id, ordinal, source_id, kind, locator, origin_path,
                    revision, content, content_sha256, observed_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    basis_id,
                    i64::try_from(ordinal)
                        .map_err(|_| StoreError::CorruptState("source ordinal overflow".into()))?,
                    source.source_id,
                    source.kind,
                    source.locator,
                    source.origin_path,
                    source.revision,
                    source.content,
                    sha256_hex(&source.content),
                    source.observed_at,
                ],
            )?;
        }
        transaction.commit()?;
        self.frozen_basis(&basis_id)
    }

    pub fn frozen_basis(&self, basis_id: &str) -> StoreResult<FrozenBasisView> {
        read_basis(&self.connection, basis_id)
    }

    pub fn steward_basis(
        &self,
        scope_id: &str,
        scope_version: u32,
    ) -> StoreResult<FrozenBasisView> {
        let basis_id: Option<String> = self
            .connection
            .query_row(
                "SELECT basis_id FROM frozen_bases
                 WHERE basis_kind = 'steward' AND scope_id = ? AND scope_version = ?",
                params![scope_id, scope_version],
                |row| row.get(0),
            )
            .optional()?;
        basis_id
            .map(|basis_id| self.frozen_basis(&basis_id))
            .transpose()?
            .ok_or_else(|| {
                StoreError::NotFound(format!(
                    "steward scope {scope_id} version {scope_version} has no registered basis"
                ))
            })
    }

    pub fn register_steward(
        &mut self,
        scope: &NewStewardScope,
        basis: &NewFrozenBasis,
    ) -> StoreResult<(StewardScopeView, FrozenBasisView)> {
        validate_identifier("scope id", &scope.scope_id)?;
        validate_text("steward party", &scope.steward_party)?;
        validate_text("scope title", &scope.title)?;
        validate_digest("descriptor digest", &scope.descriptor_sha256)?;
        if scope.version == 0 {
            return Err(StoreError::InvalidInput(
                "scope version must be greater than zero".into(),
            ));
        }
        if basis.kind != BasisKind::Steward
            || basis.scope_id.as_deref() != Some(scope.scope_id.as_str())
            || basis.scope_version != Some(scope.version)
        {
            return Err(StoreError::InvalidInput(
                "registered steward basis must name the same scope and version".into(),
            ));
        }
        let manifest_sha256 = Self::basis_manifest_sha256(basis)?;
        let charter_sha256 = scope.charter_markdown.sha256();
        let now = now_unix();
        let transaction = self.immediate()?;
        let existing_scope = read_scope_optional(&transaction, &scope.scope_id, scope.version)?;
        let existing_basis_id: Option<String> = transaction
            .query_row(
                "SELECT basis_id FROM frozen_bases
                 WHERE basis_kind = 'steward' AND scope_id = ? AND scope_version = ?",
                params![scope.scope_id, scope.version],
                |row| row.get(0),
            )
            .optional()?;
        match (existing_scope, existing_basis_id) {
            (Some(existing_scope), Some(existing_basis_id)) => {
                let existing_basis = read_basis(&transaction, &existing_basis_id)?;
                if existing_scope.steward_party != scope.steward_party
                    || existing_scope.title != scope.title
                    || existing_scope.charter_markdown != scope.charter_markdown
                    || existing_scope.descriptor_sha256 != scope.descriptor_sha256
                    || existing_basis.manifest_sha256 != manifest_sha256
                {
                    return Err(StoreError::Conflict(format!(
                        "steward scope {} version {} is already registered differently",
                        scope.scope_id, scope.version
                    )));
                }
                transaction.commit()?;
                return Ok((existing_scope, existing_basis));
            }
            (Some(_), None) | (None, Some(_)) => {
                return Err(StoreError::CorruptState(format!(
                    "steward scope {} version {} is only partially registered",
                    scope.scope_id, scope.version
                )));
            }
            (None, None) => {}
        }
        transaction.execute(
            "INSERT INTO steward_scopes (
                scope_id, version, steward_party, title, charter_markdown,
                charter_sha256, descriptor_sha256, recorded_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                scope.scope_id,
                scope.version,
                scope.steward_party,
                scope.title,
                scope.charter_markdown.as_bytes(),
                charter_sha256,
                scope.descriptor_sha256,
                now,
            ],
        )?;
        let basis_id = new_id("bas");
        insert_frozen_basis(&transaction, &basis_id, basis, &manifest_sha256, now)?;
        transaction.commit()?;
        Ok((
            self.steward_scope(&scope.scope_id, scope.version)?,
            self.frozen_basis(&basis_id)?,
        ))
    }

    pub fn freeze_candidate_basis(
        &mut self,
        basis: &NewFrozenBasis,
    ) -> StoreResult<FrozenBasisView> {
        if basis.kind != BasisKind::Candidate {
            return Err(StoreError::InvalidInput(
                "candidate registration requires a candidate basis".into(),
            ));
        }
        self.freeze_basis(basis)
    }

    pub fn record_basis_verification(
        &mut self,
        basis_id: &str,
        observed_manifest_sha256: Option<&str>,
        detail_markdown: Option<&OpaqueMarkdown>,
    ) -> StoreResult<BasisVerificationView> {
        if let Some(digest) = observed_manifest_sha256 {
            validate_digest("observed basis digest", digest)?;
        }
        let transaction = self.immediate()?;
        let view = insert_basis_verification(
            &transaction,
            basis_id,
            observed_manifest_sha256,
            detail_markdown,
        )?;
        transaction.commit()?;
        Ok(view)
    }

    pub fn create_integration(
        &mut self,
        entrant_party: &str,
        title: &str,
        context_markdown: Option<&OpaqueMarkdown>,
    ) -> StoreResult<IntegrationView> {
        validate_text("entrant party", entrant_party)?;
        validate_text("integration title", title)?;
        let integration_id = new_id("int");
        let now = now_unix();
        let context_sha256 = context_markdown.map(OpaqueMarkdown::sha256);
        let transaction = self.immediate()?;
        transaction.execute(
            "INSERT INTO integrations (
                integration_id, entrant_party, title, context_markdown,
                context_sha256, created_at
             ) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                integration_id,
                entrant_party,
                title,
                context_markdown.map(OpaqueMarkdown::as_bytes),
                context_sha256,
                now,
            ],
        )?;
        transaction.execute(
            "INSERT INTO integration_events (
                integration_id, ordinal, kind, recorded_at
             ) VALUES (?, 1, 'opened', ?)",
            params![integration_id, now],
        )?;
        transaction.commit()?;
        self.integration(&integration_id)
    }

    pub fn integration(&self, integration_id: &str) -> StoreResult<IntegrationView> {
        let row = self.connection.query_row(
            "SELECT integration_id, entrant_party, title, context_markdown,
                    context_sha256, created_at
             FROM integrations WHERE integration_id = ?",
            [integration_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<Vec<u8>>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        );
        match row {
            Ok((id, entrant, title, context, digest, created_at)) => Ok(IntegrationView {
                integration_id: id,
                entrant_party: entrant,
                title,
                context_markdown: decode_optional_markdown(context)?,
                context_sha256: digest,
                created_at,
            }),
            Err(rusqlite::Error::QueryReturnedNoRows) => Err(StoreError::NotFound(format!(
                "integration {integration_id} does not exist"
            ))),
            Err(error) => Err(error.into()),
        }
    }

    fn immediate(&mut self) -> StoreResult<Transaction<'_>> {
        Ok(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }
}

#[cfg(test)]
#[allow(clippy::too_many_lines)]
mod tests {
    use super::*;

    fn markdown(text: &str) -> OpaqueMarkdown {
        OpaqueMarkdown::new(text.as_bytes().to_vec()).expect("test Markdown")
    }

    fn registered_store() -> (Store, StewardScopeView, FrozenBasisView) {
        let mut store = Store::open_in_memory().expect("in-memory store");
        let scope = NewStewardScope {
            scope_id: "evidence.claim-admission".into(),
            version: 1,
            steward_party: "evidence-steward".into(),
            title: "Evidence admission".into(),
            charter_markdown: markdown("Review exact evidence contracts.\n"),
            descriptor_sha256: sha256_hex(b"descriptor-v1"),
        };
        let basis = NewFrozenBasis {
            kind: BasisKind::Steward,
            label: "Evidence admission implementation basis".into(),
            scope_id: Some(scope.scope_id.clone()),
            scope_version: Some(scope.version),
            verifier_version: "pratica-files-v1".into(),
            observed_at: 1,
            sources: vec![NewFrozenSource {
                source_id: "source:contract".into(),
                kind: "contract".into(),
                locator: "docs/contract.md".into(),
                origin_path: Some("/private/docs/contract.md".into()),
                revision: Some("v1".into()),
                content: b"Current contract.\n".to_vec(),
                observed_at: 1,
            }],
        };
        let registered = store
            .register_steward(&scope, &basis)
            .expect("register steward");
        let replay = store
            .register_steward(&scope, &basis)
            .expect("idempotent registration");
        assert_eq!(registered, replay);
        (store, registered.0, registered.1)
    }

    fn open_track(store: &mut Store) -> (IntegrationView, TrackView, NegotiationView) {
        let integration = store
            .create_integration("crm", "CRM design", Some(&markdown("Design only.\n")))
            .expect("integration");
        let (track, negotiation) = store
            .open_track(
                &integration.integration_id,
                "evidence.claim-admission",
                1,
                &markdown("Exact terms\r\nwith trailing space  \n"),
            )
            .expect("track");
        (integration, track, negotiation)
    }

    fn settle_negotiation(
        store: &mut Store,
        basis: &FrozenBasisView,
        negotiation: &NegotiationView,
    ) -> AgreementView {
        let head = negotiation.head.as_ref().expect("negotiation head");
        let result = store
            .apply_steward_response(
                &negotiation.negotiation_id,
                &head.offer_id,
                &BasisGuard {
                    basis_id: basis.basis_id.clone(),
                    observed_manifest_sha256: basis.manifest_sha256.clone(),
                },
                &StewardResponse::Assent {
                    review_markdown: markdown("Agreed on the frozen implementation basis.\n"),
                },
                None,
            )
            .expect("seal negotiation");
        store
            .agreement(&result.agreement_id.expect("agreement id"))
            .expect("agreement")
    }

    fn attempt_sources(basis: &FrozenBasisView) -> Vec<AttemptSourceInput> {
        basis
            .sources
            .iter()
            .map(|source| AttemptSourceInput {
                source_id: source.source_id.clone(),
                kind: source.kind.clone(),
                locator: source.locator.clone(),
                origin_path: source
                    .origin_path
                    .clone()
                    .expect("test basis has origin paths"),
                revision: source.revision.clone(),
                content: source.content.clone(),
                content_sha256: source.content_sha256.clone(),
                observed_at: source.observed_at,
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn agent_attempt(
        kind: AttemptKind,
        subject_id: &str,
        job: &str,
        expected_offer_id: Option<String>,
        expected_roster_digest: Option<String>,
        target_basis: Option<&FrozenBasisView>,
        catalog_scope: &str,
        catalog_party: &str,
        catalog_title: &str,
        catalog_basis: &FrozenBasisView,
    ) -> NewAgentAttempt {
        let request_bytes = format!("{{\"job\":\"{job}\"}}").into_bytes();
        let charter_markdown = markdown("Use only the frozen test catalog.\n");
        let basis_digest = match kind {
            AttemptKind::CompositionReview => {
                expected_roster_digest.clone().expect("composition digest")
            }
            AttemptKind::StewardResponse | AttemptKind::ConformanceReview => {
                target_basis.expect("target basis").manifest_sha256.clone()
            }
        };
        NewAgentAttempt {
            predecessor_attempt_id: None,
            kind,
            subject_id: subject_id.into(),
            requester_id: format!("requester-{job}"),
            nucleus_job_id: job.into(),
            request_sha256: sha256_hex(&request_bytes),
            request_bytes,
            toolset_name: format!("pratica/{}", kind.as_str().replace('_', "-")),
            toolset_version: 1,
            expected_offer_id,
            expected_roster_digest,
            basis_id: target_basis.map(|basis| basis.basis_id.clone()),
            basis_digest,
            catalog_scope: catalog_scope.into(),
            catalog_version: 1,
            catalog_verifier_version: "pratica-files-v1".into(),
            catalog_observed_at: catalog_basis.observed_at,
            catalog_party: catalog_party.into(),
            catalog_title: catalog_title.into(),
            catalog_charter_sha256: charter_markdown.sha256(),
            catalog_charter_markdown: charter_markdown,
            catalog_sha256: sha256_hex(format!("catalog:{job}").as_bytes()),
            sources: attempt_sources(catalog_basis),
        }
    }

    fn assert_terminal_receipt(receipt: &ToolReceiptView, kind: &str, status: &str) -> String {
        let result: serde_json::Value =
            serde_json::from_slice(&receipt.result_json).expect("terminal result JSON");
        assert_eq!(result["ok"], true);
        assert_eq!(result["recorded"]["kind"], kind);
        assert_eq!(result["recorded"]["status"], status);
        let id = result["recorded"]["id"]
            .as_str()
            .expect("recorded id")
            .to_owned();
        assert_eq!(receipt.domain_result_kind.as_deref(), Some(kind));
        assert_eq!(receipt.domain_result_id.as_deref(), Some(id.as_str()));
        id
    }

    #[test]
    fn identical_terms_are_distinct_offers_and_assent_does_not_add_one() {
        let (mut store, _, basis) = registered_store();
        let (_, _, initial) = open_track(&mut store);
        let first = initial.head.expect("initial head");
        let proposed = store
            .propose_as_entrant(
                &initial.negotiation_id,
                &first.offer_id,
                &first.terms_markdown,
            )
            .expect("repeat proposal");
        let second = proposed.negotiation.head.expect("second head");
        assert_ne!(first.offer_id, second.offer_id);
        assert_eq!(first.terms_sha256, second.terms_sha256);

        let guard = BasisGuard {
            basis_id: basis.basis_id,
            observed_manifest_sha256: basis.manifest_sha256,
        };
        let sealed = store
            .apply_steward_response(
                &initial.negotiation_id,
                &second.offer_id,
                &guard,
                &StewardResponse::Assent {
                    review_markdown: markdown("Accurate on this basis.\n"),
                },
                None,
            )
            .expect("steward assent");
        assert!(sealed.agreement_id.is_some());
        let offers = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM offers WHERE negotiation_id = ?",
                [&initial.negotiation_id],
                |row| row.get::<_, u32>(0),
            )
            .expect("offer count");
        assert_eq!(offers, 2);
    }

    #[test]
    fn counterproposal_stales_entrant_then_explicit_assent_seals() {
        let (mut store, _, basis) = registered_store();
        let (_, _, initial) = open_track(&mut store);
        let initial_head = initial.head.expect("head");
        let guard = BasisGuard {
            basis_id: basis.basis_id,
            observed_manifest_sha256: basis.manifest_sha256,
        };
        let countered = store
            .apply_steward_response(
                &initial.negotiation_id,
                &initial_head.offer_id,
                &guard,
                &StewardResponse::Counterproposal {
                    terms_markdown: markdown("Rectified complete terms.\n"),
                    review_markdown: markdown("One expectation required correction.\n"),
                },
                None,
            )
            .expect("counterproposal");
        assert_eq!(
            countered.negotiation.entrant.status,
            AssentStatus::StaleTerms
        );
        assert_eq!(countered.negotiation.steward.status, AssentStatus::Current);
        let counter_head = countered.negotiation.head.expect("counter head");
        let sealed = store
            .assent_as_entrant(
                &initial.negotiation_id,
                &counter_head.offer_id,
                Some(&guard),
            )
            .expect("entrant assent");
        assert_eq!(sealed.negotiation.status, NegotiationStatus::Sealed);
        assert!(sealed.agreement_id.is_some());
        let offers = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM offers WHERE negotiation_id = ?",
                [&initial.negotiation_id],
                |row| row.get::<_, u32>(0),
            )
            .expect("offer count");
        assert_eq!(offers, 2);
    }

    #[test]
    fn stale_basis_cannot_mutate_negotiation() {
        let (mut store, _, basis) = registered_store();
        let (_, _, initial) = open_track(&mut store);
        let head = initial.head.expect("head");
        let before = store
            .negotiation_history(&initial.negotiation_id)
            .expect("history")
            .len();
        let error = store
            .apply_steward_response(
                &initial.negotiation_id,
                &head.offer_id,
                &BasisGuard {
                    basis_id: basis.basis_id,
                    observed_manifest_sha256: "0".repeat(64),
                },
                &StewardResponse::Assent {
                    review_markdown: markdown("Must not commit.\n"),
                },
                None,
            )
            .expect_err("stale guard");
        assert_eq!(error.kind(), StoreErrorKind::Stale);
        assert_eq!(
            store
                .negotiation_history(&initial.negotiation_id)
                .expect("history")
                .len(),
            before
        );
    }

    #[test]
    fn later_steward_block_revokes_prior_assent() {
        let (mut store, _, basis) = registered_store();
        let (_, _, initial) = open_track(&mut store);
        let head = initial.head.expect("head");
        store
            .withdraw_entrant_assent(&initial.negotiation_id, &head.offer_id)
            .expect("withdraw entrant assent");
        let guard = BasisGuard {
            basis_id: basis.basis_id,
            observed_manifest_sha256: basis.manifest_sha256,
        };
        let assented = store
            .apply_steward_response(
                &initial.negotiation_id,
                &head.offer_id,
                &guard,
                &StewardResponse::Assent {
                    review_markdown: markdown("Steward initially assents.\n"),
                },
                None,
            )
            .expect("steward assent without entrant assent");
        assert!(assented.agreement_id.is_none());
        let blocked = store
            .apply_steward_response(
                &initial.negotiation_id,
                &head.offer_id,
                &guard,
                &StewardResponse::Blocked {
                    review_markdown: markdown("A later finding blocks agreement.\n"),
                },
                None,
            )
            .expect("later steward block");
        assert_eq!(blocked.negotiation.steward.status, AssentStatus::Blocked);
        let entrant = store
            .assent_as_entrant(&initial.negotiation_id, &head.offer_id, Some(&guard))
            .expect("entrant assent remains an open negotiation");
        assert!(entrant.agreement_id.is_none());
        assert_eq!(entrant.negotiation.status, NegotiationStatus::Open);
        assert_eq!(entrant.negotiation.steward.status, AssentStatus::Blocked);
    }

    #[test]
    fn amendment_keeps_predecessor_current_until_successor_seals() {
        let (mut store, _, basis) = registered_store();
        let (integration, track, initial) = open_track(&mut store);
        let head = initial.head.expect("head");
        let guard = BasisGuard {
            basis_id: basis.basis_id,
            observed_manifest_sha256: basis.manifest_sha256,
        };
        let settled = store
            .apply_steward_response(
                &initial.negotiation_id,
                &head.offer_id,
                &guard,
                &StewardResponse::Assent {
                    review_markdown: markdown("Agreed.\n"),
                },
                None,
            )
            .expect("seal");
        let agreement_id = settled.agreement_id.expect("agreement");
        let (_, _, composition_digest) = store
            .composition_basis(&integration.integration_id)
            .expect("composition basis");
        store
            .record_composition_review(
                &integration.integration_id,
                &composition_digest,
                CompositionOutcome::Compatible,
                &markdown("No cross-track conflicts.\n"),
                None,
            )
            .expect("composition review");
        store
            .open_amendment(&agreement_id, &markdown("Proposed successor terms.\n"))
            .expect("amendment");
        let status = store
            .integration_status(&integration.integration_id)
            .expect("status");
        assert!(status.ready);
        assert!(status.tracks[0].renegotiating);
        assert_eq!(
            status.tracks[0]
                .active_agreement
                .as_ref()
                .expect("predecessor")
                .agreement_id,
            agreement_id
        );
        assert_eq!(status.tracks[0].track.track_id, track.track_id);
    }

    #[test]
    fn amendment_is_single_flight_and_seal_rechecks_its_predecessor() {
        let (mut store, _, basis) = registered_store();
        let (_, track, initial) = open_track(&mut store);
        let predecessor = settle_negotiation(&mut store, &basis, &initial);
        let first = store
            .open_amendment(&predecessor.agreement_id, &markdown("First successor.\n"))
            .expect("first amendment");
        let conflict = store
            .open_amendment(
                &predecessor.agreement_id,
                &markdown("Parallel successor.\n"),
            )
            .expect_err("parallel amendment must be rejected");
        assert_eq!(conflict.kind(), StoreErrorKind::Conflict);

        let second_id = new_id("neg");
        let second_offer_id = new_id("off");
        let now = now_unix();
        let transaction = store.immediate().expect("transaction");
        insert_negotiation(
            &transaction,
            &second_id,
            &track.track_id,
            NegotiationKind::Amendment,
            Some(&predecessor.agreement_id),
            now,
        )
        .expect("simulate a concurrent amendment admitted by an older writer");
        insert_offer_event(
            &transaction,
            &second_offer_id,
            &second_id,
            PartyRole::Entrant,
            &markdown("Simulated racing successor.\n"),
            None,
            now,
        )
        .expect("racing offer");
        transaction.commit().expect("commit simulated race");

        let guard = BasisGuard {
            basis_id: basis.basis_id,
            observed_manifest_sha256: basis.manifest_sha256,
        };
        let first_head = first.head.expect("first head");
        store
            .apply_steward_response(
                &first.negotiation_id,
                &first_head.offer_id,
                &guard,
                &StewardResponse::Assent {
                    review_markdown: markdown("First successor wins.\n"),
                },
                None,
            )
            .expect("seal first successor");
        let before = store
            .negotiation_history(&second_id)
            .expect("racing history")
            .len();
        let stale = store
            .apply_steward_response(
                &second_id,
                &second_offer_id,
                &guard,
                &StewardResponse::Assent {
                    review_markdown: markdown("Must lose the race.\n"),
                },
                None,
            )
            .expect_err("second successor cannot seal");
        assert_eq!(stale.kind(), StoreErrorKind::Stale);
        assert_eq!(
            store
                .negotiation_history(&second_id)
                .expect("racing history")
                .len(),
            before
        );
        assert!(
            store
                .connection
                .query_row(
                    "SELECT NOT EXISTS(SELECT 1 FROM agreements WHERE negotiation_id = ?)",
                    [&second_id],
                    |row| row.get::<_, bool>(0),
                )
                .expect("no racing agreement")
        );
    }

    #[test]
    fn agreement_verification_checks_integrity_before_appending_freshness() {
        let (mut store, _, basis) = registered_store();
        let (_, _, initial) = open_track(&mut store);
        let agreement = settle_negotiation(&mut store, &basis, &initial);
        let before: u32 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM basis_verifications WHERE basis_id = ?",
                [&basis.basis_id],
                |row| row.get(0),
            )
            .expect("verification count");
        store
            .connection
            .execute_batch("DROP TRIGGER offers_no_update")
            .expect("simulate out-of-band corruption");
        store
            .connection
            .execute(
                "UPDATE offers SET terms_sha256 = ? WHERE offer_id = ?",
                params!["0".repeat(64), agreement.offer.offer_id],
            )
            .expect("corrupt offer digest");
        let error = store
            .verify_agreement(&agreement.agreement_id, Some(&basis.manifest_sha256), None)
            .expect_err("corrupt agreement must not be verified");
        assert_eq!(error.kind(), StoreErrorKind::CorruptState);
        let after: u32 = store
            .connection
            .query_row(
                "SELECT COUNT(*) FROM basis_verifications WHERE basis_id = ?",
                [&basis.basis_id],
                |row| row.get(0),
            )
            .expect("verification count");
        assert_eq!(after, before);
    }

    #[test]
    fn domain_commit_stays_recoverable_until_nucleus_is_terminal() {
        let (mut store, scope, basis) = registered_store();
        let (_, _, negotiation) = open_track(&mut store);
        let head = negotiation.head.expect("head");
        let source = AttemptSourceInput {
            source_id: "source:contract".into(),
            kind: "contract".into(),
            locator: "docs/contract.md".into(),
            origin_path: "/private/docs/contract.md".into(),
            revision: Some("v1".into()),
            content: b"Current contract.\n".to_vec(),
            content_sha256: sha256_hex(b"Current contract.\n"),
            observed_at: 1,
        };
        let make_attempt = |job: &str| {
            let request_bytes = format!("{{\"job\":\"{job}\"}}").into_bytes();
            NewAgentAttempt {
                predecessor_attempt_id: None,
                kind: AttemptKind::StewardResponse,
                subject_id: negotiation.negotiation_id.clone(),
                requester_id: format!("negotiation-{}", negotiation.negotiation_id),
                nucleus_job_id: job.into(),
                request_sha256: sha256_hex(&request_bytes),
                request_bytes,
                toolset_name: "pratica/steward-response".into(),
                toolset_version: 1,
                expected_offer_id: Some(head.offer_id.clone()),
                expected_roster_digest: None,
                basis_id: Some(basis.basis_id.clone()),
                basis_digest: basis.manifest_sha256.clone(),
                catalog_scope: scope.scope_id.clone(),
                catalog_version: scope.version,
                catalog_verifier_version: "pratica-files-v1".into(),
                catalog_observed_at: 1,
                catalog_party: scope.steward_party.clone(),
                catalog_title: scope.title.clone(),
                catalog_charter_sha256: scope.charter_sha256.clone(),
                catalog_charter_markdown: scope.charter_markdown.clone(),
                catalog_sha256: sha256_hex(b"catalog"),
                sources: vec![source.clone()],
            }
        };
        let first_input = make_attempt("pratica-steward-first");
        let first = store
            .begin_or_resume_attempt(&first_input)
            .expect("attempt");
        assert!(attempt_matches_input(&first, &first_input));
        assert_eq!(first.catalog_verifier_version, "pratica-files-v1");
        assert_eq!(first.catalog_observed_at, 1);
        assert_eq!(first.sources[0].observed_at, 1);
        let receipt = store
            .commit_steward_tool_response(
                &first.attempt_id,
                "call-1",
                &sha256_hex(b"arguments"),
                &basis.manifest_sha256,
                &StewardResponse::Blocked {
                    review_markdown: markdown("Needs entrant authority.\n"),
                },
                &[],
            )
            .expect("commit blocked response");
        assert_eq!(
            receipt.domain_result_kind.as_deref(),
            Some("steward_response")
        );
        let recorded: serde_json::Value =
            serde_json::from_slice(&receipt.result_json).expect("canonical result JSON");
        assert_eq!(recorded["ok"], true);
        assert_eq!(recorded["recorded"]["kind"], "steward_response");
        assert_eq!(
            recorded["recorded"]["id"].as_str(),
            receipt.domain_result_id.as_deref()
        );
        assert_eq!(recorded["recorded"]["status"], "blocked");

        let committed_unacknowledged = store.attempt(&first.attempt_id).expect("attempt");
        assert!(committed_unacknowledged.active);
        assert_eq!(
            store
                .begin_or_resume_attempt(&make_attempt("pratica-steward-first"))
                .expect("resume committed but unacknowledged attempt")
                .attempt_id,
            first.attempt_id
        );
        store
            .advance_attempt_tool_after(&first.attempt_id, 1)
            .expect("persist receipt acknowledgement");
        assert!(store.attempt(&first.attempt_id).expect("attempt").active);
        store
            .mark_attempt_runtime_state(
                &first.attempt_id,
                RuntimeState::Completed,
                Some("Nucleus completed after accepting the result"),
            )
            .expect("record Nucleus terminal acknowledgement");
        assert!(!store.attempt(&first.attempt_id).expect("attempt").active);
        let second_input = make_attempt("pratica-steward-second");
        let second = store
            .begin_or_resume_attempt(&second_input)
            .expect("new command attempt");
        assert_ne!(first.attempt_id, second.attempt_id);
        assert!(second.active);
        store
            .propose_as_entrant(
                &negotiation.negotiation_id,
                &head.offer_id,
                &markdown("A new entrant head races the old steward request.\n"),
            )
            .expect("move negotiation head");
        let before = store
            .negotiation_history(&negotiation.negotiation_id)
            .expect("history")
            .len();
        let stale = store
            .commit_steward_tool_response(
                &second.attempt_id,
                "call-stale-head",
                &sha256_hex(b"stale arguments"),
                &basis.manifest_sha256,
                &StewardResponse::Assent {
                    review_markdown: markdown("Must not be recorded.\n"),
                },
                &[],
            )
            .expect_err("stale expected head");
        assert_eq!(stale.kind(), StoreErrorKind::Stale);
        assert_eq!(
            store
                .negotiation_history(&negotiation.negotiation_id)
                .expect("history")
                .len(),
            before
        );
        let unchanged = store.attempt(&second.attempt_id).expect("attempt");
        assert!(unchanged.active);
        assert!(unchanged.domain_result_id.is_none());
        assert!(
            store
                .tool_receipt(&second.nucleus_job_id, "call-stale-head")
                .expect("receipt lookup")
                .is_none()
        );
    }

    #[test]
    fn composition_commit_is_canonical_advisory_and_stale_fenced() {
        let (mut store, scope, basis) = registered_store();
        let (integration, track, initial) = open_track(&mut store);
        let agreement = settle_negotiation(&mut store, &basis, &initial);
        let (_, references, composition_digest) = store
            .composition_basis(&integration.integration_id)
            .expect("composition basis");
        assert_eq!(references.len(), 1);
        let input = agent_attempt(
            AttemptKind::CompositionReview,
            &integration.integration_id,
            "pratica-composition-first",
            None,
            Some(composition_digest.clone()),
            None,
            &scope.scope_id,
            &scope.steward_party,
            "Composition catalog",
            &basis,
        );
        let attempt = store
            .begin_or_resume_attempt(&input)
            .expect("composition attempt");
        assert!(attempt_matches_input(&attempt, &input));
        let evidence_ref = "source:contract#sha256".to_owned();
        store
            .record_tool_receipt(
                &attempt.attempt_id,
                "source-read",
                &sha256_hex(b"source read arguments"),
                br#"{"ok":true,"data":{"text":"Current contract."}}"#,
                false,
                std::slice::from_ref(&evidence_ref),
                None,
            )
            .expect("source receipt");
        let event_count_before = store
            .negotiation_history(&initial.negotiation_id)
            .expect("history")
            .len();
        let receipt = store
            .commit_composition_tool_response(
                &attempt.attempt_id,
                "submit-composition",
                &sha256_hex(b"composition arguments"),
                &[(basis.basis_id.clone(), basis.manifest_sha256.clone())],
                CompositionOutcome::Compatible,
                &markdown("The active agreements compose without contradiction.\n"),
                std::slice::from_ref(&evidence_ref),
            )
            .expect("composition commit");
        let review_id = assert_terminal_receipt(&receipt, "composition_review", "compatible");
        assert_eq!(receipt.emitted_source_refs, vec![evidence_ref]);
        let review = store
            .composition_review(&review_id)
            .expect("composition review");
        assert_eq!(
            review.attempt_id.as_deref(),
            Some(attempt.attempt_id.as_str())
        );
        assert_eq!(review.agreements[0].agreement_id, agreement.agreement_id);
        assert!(!review.stale);
        assert_eq!(
            store
                .negotiation_history(&initial.negotiation_id)
                .expect("history")
                .len(),
            event_count_before
        );
        assert_eq!(
            store
                .attempt_receipts(&attempt.attempt_id)
                .expect("attempt receipts")
                .len(),
            2
        );
        assert!(store.attempt(&attempt.attempt_id).expect("attempt").active);
        store
            .mark_attempt_runtime_state(&attempt.attempt_id, RuntimeState::Completed, None)
            .expect("terminal composition job");

        let (_, _, next_digest) = store
            .composition_basis(&integration.integration_id)
            .expect("next composition basis");
        let stale_input = agent_attempt(
            AttemptKind::CompositionReview,
            &integration.integration_id,
            "pratica-composition-stale",
            None,
            Some(next_digest),
            None,
            &scope.scope_id,
            &scope.steward_party,
            "Composition catalog",
            &basis,
        );
        let stale_attempt = store
            .begin_or_resume_attempt(&stale_input)
            .expect("second composition attempt");
        store
            .retire_track(
                &track.track_id,
                "Remove the settled track from this integration",
            )
            .expect("retire track");
        let reviews_before: u32 = store
            .connection
            .query_row("SELECT COUNT(*) FROM composition_reviews", [], |row| {
                row.get(0)
            })
            .expect("review count");
        let stale = store
            .commit_composition_tool_response(
                &stale_attempt.attempt_id,
                "submit-stale-composition",
                &sha256_hex(b"stale composition arguments"),
                &[],
                CompositionOutcome::Compatible,
                &markdown("Must not be recorded.\n"),
                &[],
            )
            .expect_err("changed roster must fence composition");
        assert_eq!(stale.kind(), StoreErrorKind::Stale);
        let reviews_after: u32 = store
            .connection
            .query_row("SELECT COUNT(*) FROM composition_reviews", [], |row| {
                row.get(0)
            })
            .expect("review count");
        assert_eq!(reviews_after, reviews_before);
        assert!(
            store
                .attempt(&stale_attempt.attempt_id)
                .expect("stale attempt")
                .domain_result_id
                .is_none()
        );
    }

    #[test]
    fn conformance_commit_is_canonical_non_mutating_and_basis_fenced() {
        let (mut store, _, steward_basis) = registered_store();
        let (_, _, initial) = open_track(&mut store);
        let agreement = settle_negotiation(&mut store, &steward_basis, &initial);
        let candidate = store
            .freeze_candidate_basis(&NewFrozenBasis {
                kind: BasisKind::Candidate,
                label: "CRM candidate".into(),
                scope_id: None,
                scope_version: None,
                verifier_version: "pratica-files-v1".into(),
                observed_at: 2,
                sources: vec![NewFrozenSource {
                    source_id: "candidate:crm".into(),
                    kind: "implementation".into(),
                    locator: "crm/model.rs".into(),
                    origin_path: Some("/private/crm/model.rs".into()),
                    revision: Some("candidate-v1".into()),
                    content: b"struct Contact;\n".to_vec(),
                    observed_at: 2,
                }],
            })
            .expect("candidate basis");
        let input = agent_attempt(
            AttemptKind::ConformanceReview,
            &agreement.agreement_id,
            "pratica-conformance-first",
            None,
            None,
            Some(&candidate),
            "crm.candidate",
            "crm",
            "CRM candidate",
            &candidate,
        );
        let attempt = store
            .begin_or_resume_attempt(&input)
            .expect("conformance attempt");
        let evidence_ref = "candidate:crm#L1".to_owned();
        store
            .record_tool_receipt(
                &attempt.attempt_id,
                "candidate-read",
                &sha256_hex(b"candidate read arguments"),
                br#"{"ok":true,"data":{"text":"struct Contact;"}}"#,
                false,
                std::slice::from_ref(&evidence_ref),
                None,
            )
            .expect("candidate source receipt");
        let event_count_before = store
            .negotiation_history(&agreement.negotiation_id)
            .expect("history")
            .len();
        let receipt = store
            .commit_conformance_tool_response(
                &attempt.attempt_id,
                "submit-conformance",
                &sha256_hex(b"conformance arguments"),
                &candidate.manifest_sha256,
                ConformanceOutcome::DoesNotConform,
                &markdown("The candidate omits required contract behavior.\n"),
                std::slice::from_ref(&evidence_ref),
            )
            .expect("conformance commit");
        let review_id = assert_terminal_receipt(&receipt, "conformance_review", "does_not_conform");
        let review = store
            .conformance_review(&review_id)
            .expect("conformance review");
        assert_eq!(
            review.attempt_id.as_deref(),
            Some(attempt.attempt_id.as_str())
        );
        assert_eq!(review.candidate_freshness, BasisFreshness::Fresh);
        assert_eq!(
            store
                .negotiation_history(&agreement.negotiation_id)
                .expect("history")
                .len(),
            event_count_before
        );
        assert_eq!(
            store
                .agreement(&agreement.agreement_id)
                .expect("agreement")
                .offer,
            agreement.offer
        );
        assert!(store.attempt(&attempt.attempt_id).expect("attempt").active);
        store
            .mark_attempt_runtime_state(&attempt.attempt_id, RuntimeState::Completed, None)
            .expect("terminal conformance job");

        let stale_input = agent_attempt(
            AttemptKind::ConformanceReview,
            &agreement.agreement_id,
            "pratica-conformance-stale",
            None,
            None,
            Some(&candidate),
            "crm.candidate",
            "crm",
            "CRM candidate",
            &candidate,
        );
        let stale_attempt = store
            .begin_or_resume_attempt(&stale_input)
            .expect("stale candidate attempt");
        let reviews_before: u32 = store
            .connection
            .query_row("SELECT COUNT(*) FROM conformance_reviews", [], |row| {
                row.get(0)
            })
            .expect("review count");
        let stale = store
            .commit_conformance_tool_response(
                &stale_attempt.attempt_id,
                "submit-stale-conformance",
                &sha256_hex(b"stale conformance arguments"),
                &"0".repeat(64),
                ConformanceOutcome::Conforms,
                &markdown("Must not be recorded.\n"),
                &[],
            )
            .expect_err("changed candidate basis must be fenced");
        assert_eq!(stale.kind(), StoreErrorKind::Stale);
        let reviews_after: u32 = store
            .connection
            .query_row("SELECT COUNT(*) FROM conformance_reviews", [], |row| {
                row.get(0)
            })
            .expect("review count");
        assert_eq!(reviews_after, reviews_before);
        assert!(
            store
                .attempt(&stale_attempt.attempt_id)
                .expect("stale attempt")
                .domain_result_id
                .is_none()
        );
    }
}

fn configure_connection(connection: &Connection) -> StoreResult<()> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(())
}

const REQUIRED_TABLES: &[&str] = &[
    "pratica_meta",
    "steward_scopes",
    "frozen_bases",
    "frozen_basis_sources",
    "basis_verifications",
    "integrations",
    "integration_tracks",
    "integration_events",
    "negotiations",
    "offers",
    "negotiation_events",
    "agreements",
    "agent_attempts",
    "attempt_sources",
    "tool_receipts",
    "tool_receipt_source_refs",
    "composition_reviews",
    "composition_review_agreements",
    "conformance_reviews",
];

const REQUIRED_INDEXES: &[&str] = &[
    "frozen_bases_one_per_steward_version",
    "basis_verifications_latest",
    "negotiation_events_projection",
    "agreements_offer",
    "agent_attempts_one_active",
];

const IMMUTABLE_TABLES: &[&str] = &[
    "steward_scopes",
    "frozen_bases",
    "frozen_basis_sources",
    "basis_verifications",
    "integrations",
    "integration_tracks",
    "integration_events",
    "negotiations",
    "offers",
    "negotiation_events",
    "agreements",
    "attempt_sources",
    "tool_receipts",
    "tool_receipt_source_refs",
    "composition_reviews",
    "composition_review_agreements",
    "conformance_reviews",
];

fn require_schema_objects(connection: &Connection) -> StoreResult<()> {
    for (kind, names) in [("table", REQUIRED_TABLES), ("index", REQUIRED_INDEXES)] {
        for name in names {
            let exists: bool = connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master WHERE type = ? AND name = ?
                 )",
                params![kind, name],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(StoreError::CorruptState(format!(
                    "required SQLite {kind} {name} is missing"
                )));
            }
        }
    }
    for table in IMMUTABLE_TABLES {
        for suffix in ["no_update", "no_delete"] {
            let trigger = format!("{table}_{suffix}");
            let exists: bool = connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM sqlite_master WHERE type = 'trigger' AND name = ?
                 )",
                [&trigger],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(StoreError::CorruptState(format!(
                    "required SQLite trigger {trigger} is missing"
                )));
            }
        }
    }
    for trigger in [
        "agent_attempts_identity_immutable",
        "agent_attempts_no_delete",
    ] {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master WHERE type = 'trigger' AND name = ?
             )",
            [trigger],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StoreError::CorruptState(format!(
                "required SQLite trigger {trigger} is missing"
            )));
        }
    }
    Ok(())
}

fn verify_digest_rows(connection: &Connection, query: &str, label: &str) -> StoreResult<()> {
    let mut statement = connection.prepare(query)?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Vec<u8>>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (id, bytes, stored_digest) = row?;
        if sha256_hex(&bytes) != stored_digest {
            return Err(StoreError::CorruptState(format!(
                "{label} {id} digest does not match its exact bytes"
            )));
        }
    }
    Ok(())
}

fn verify_stored_digests(store: &Store) -> StoreResult<()> {
    let connection = &store.connection;
    verify_digest_rows(
        connection,
        "SELECT scope_id || ':' || version, charter_markdown, charter_sha256 FROM steward_scopes",
        "steward charter",
    )?;
    verify_digest_rows(
        connection,
        "SELECT basis_id || ':' || source_id, content, content_sha256 FROM frozen_basis_sources",
        "frozen basis source",
    )?;
    verify_digest_rows(
        connection,
        "SELECT offer_id, terms_markdown, terms_sha256 FROM offers",
        "offer",
    )?;
    verify_digest_rows(
        connection,
        "SELECT integration_id, context_markdown, context_sha256
         FROM integrations WHERE context_markdown IS NOT NULL",
        "integration context",
    )?;
    verify_digest_rows(
        connection,
        "SELECT attempt_id, request_bytes, request_sha256 FROM agent_attempts",
        "attempt request",
    )?;
    verify_digest_rows(
        connection,
        "SELECT attempt_id, catalog_charter_markdown, catalog_charter_sha256 FROM agent_attempts",
        "attempt charter",
    )?;
    verify_digest_rows(
        connection,
        "SELECT attempt_id || ':' || source_id, content, content_sha256 FROM attempt_sources",
        "attempt source",
    )?;
    verify_digest_rows(
        connection,
        "SELECT review_id, review_markdown, review_sha256 FROM composition_reviews",
        "composition review",
    )?;
    verify_digest_rows(
        connection,
        "SELECT review_id, review_markdown, review_sha256 FROM conformance_reviews",
        "conformance review",
    )?;
    let mut statement =
        connection.prepare("SELECT basis_id FROM frozen_bases ORDER BY basis_id")?;
    let basis_ids = collect_rows(statement.query_map([], |row| row.get::<_, String>(0))?)?;
    for basis_id in basis_ids {
        let basis = store.frozen_basis(&basis_id)?;
        let input = NewFrozenBasis {
            kind: basis.kind,
            label: basis.label,
            scope_id: basis.scope_id,
            scope_version: basis.scope_version,
            verifier_version: basis.verifier_version,
            observed_at: basis.observed_at,
            sources: basis
                .sources
                .into_iter()
                .map(|source| NewFrozenSource {
                    source_id: source.source_id,
                    kind: source.kind,
                    locator: source.locator,
                    origin_path: source.origin_path,
                    revision: source.revision,
                    content: source.content,
                    observed_at: source.observed_at,
                })
                .collect(),
        };
        if Store::basis_manifest_sha256(&input)? != basis.manifest_sha256 {
            return Err(StoreError::CorruptState(format!(
                "frozen basis {basis_id} manifest digest is invalid"
            )));
        }
    }
    let mut receipts = connection.prepare("SELECT receipt_id, result_json FROM tool_receipts")?;
    for row in receipts.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    })? {
        let (receipt_id, bytes) = row?;
        if serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
            return Err(StoreError::CorruptState(format!(
                "tool receipt {receipt_id} contains invalid JSON"
            )));
        }
    }
    Ok(())
}

fn verify_protocol_invariants(connection: &Connection) -> StoreResult<()> {
    let incomplete_scope: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM steward_scopes s
            LEFT JOIN frozen_bases b
              ON b.scope_id = s.scope_id AND b.scope_version = s.version
             AND b.basis_kind = 'steward'
            GROUP BY s.scope_id, s.version HAVING COUNT(b.basis_id) != 1
         )",
        [],
        |row| row.get(0),
    )?;
    if incomplete_scope {
        return Err(StoreError::CorruptState(
            "a steward scope does not bind exactly one frozen basis".into(),
        ));
    }
    let invalid_attempt: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM agent_attempts
            WHERE (domain_result_kind IS NULL) != (domain_result_id IS NULL)
               OR (domain_result_id IS NOT NULL AND NOT EXISTS (
                    SELECT 1 FROM tool_receipts r
                    WHERE r.attempt_id = agent_attempts.attempt_id
                      AND r.domain_result_kind = agent_attempts.domain_result_kind
                      AND r.domain_result_id = agent_attempts.domain_result_id))
               OR (active = 1 AND runtime_state IN (
                    'completed', 'failed', 'cancelled', 'lost', 'timed_out'))
               OR (admitted = 1 AND (
                    accepted_job_id != nucleus_job_id
                    OR accepted_request_sha256 != request_sha256))
               OR (admitted = 0 AND (
                    accepted_job_id IS NOT NULL OR accepted_request_sha256 IS NOT NULL))
         )",
        [],
        |row| row.get(0),
    )?;
    if invalid_attempt {
        return Err(StoreError::CorruptState(
            "an attempt correlation or lifecycle invariant is broken".into(),
        ));
    }
    let invalid_receipt: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM tool_receipts r
            JOIN agent_attempts a ON a.attempt_id = r.attempt_id
            WHERE r.nucleus_job_id != a.nucleus_job_id
         )",
        [],
        |row| row.get(0),
    )?;
    if invalid_receipt {
        return Err(StoreError::CorruptState(
            "a tool receipt has the wrong Nucleus correlation".into(),
        ));
    }
    let invalid_review_ref: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM composition_review_agreements r
            JOIN agreements a ON a.agreement_id = r.agreement_id
            JOIN offers o ON o.offer_id = a.offer_id
            WHERE r.terms_sha256 != o.terms_sha256 OR r.basis_id != a.basis_id
         )",
        [],
        |row| row.get(0),
    )?;
    if invalid_review_ref {
        return Err(StoreError::CorruptState(
            "a composition review does not pin its exact agreement".into(),
        ));
    }
    let invalid_conformance: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM conformance_reviews c
            JOIN frozen_bases b ON b.basis_id = c.candidate_basis_id
            WHERE b.basis_kind != 'candidate'
         )",
        [],
        |row| row.get(0),
    )?;
    if invalid_conformance {
        return Err(StoreError::CorruptState(
            "a conformance review does not use a candidate basis".into(),
        ));
    }
    let mut statement = connection.prepare("SELECT agreement_id FROM agreements")?;
    let agreement_ids = collect_rows(statement.query_map([], |row| row.get::<_, String>(0))?)?;
    for agreement_id in agreement_ids {
        verify_agreement_integrity(connection, &agreement_id)?;
    }
    Ok(())
}

fn require_schema(connection: &Connection) -> StoreResult<()> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != SCHEMA_VERSION {
        return Err(StoreError::Conflict(format!(
            "unsupported Pratica schema version {version}; expected {SCHEMA_VERSION}"
        )));
    }
    let meta: String = connection.query_row(
        "SELECT value FROM pratica_meta WHERE key = 'schema_version'",
        [],
        |row| row.get(0),
    )?;
    if meta != SCHEMA_VERSION.to_string() {
        return Err(StoreError::CorruptState(
            "Pratica schema markers disagree".into(),
        ));
    }
    Ok(())
}

fn require_private_directory(path: &Path) -> StoreResult<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(StoreError::InvalidInput(format!(
            "{} is not a real directory",
            path.display()
        )));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(StoreError::InvalidInput(format!(
            "{} must not be accessible by group or other users",
            path.display()
        )));
    }
    Ok(())
}

fn require_existing_private_file(path: &Path) -> StoreResult<()> {
    if !path.exists() {
        return Err(StoreError::NotFound(format!(
            "database {} does not exist; initialize it explicitly",
            path.display()
        )));
    }
    if let Some(parent) = path.parent() {
        require_private_directory(parent)?;
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(StoreError::InvalidInput(format!(
            "{} is not a real file",
            path.display()
        )));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(StoreError::InvalidInput(format!(
            "{} must not be accessible by group or other users",
            path.display()
        )));
    }
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        if sidecar.exists() {
            let sidecar_metadata = fs::symlink_metadata(&sidecar)?;
            if !sidecar_metadata.file_type().is_file()
                || sidecar_metadata.file_type().is_symlink()
                || sidecar_metadata.permissions().mode() & 0o077 != 0
            {
                return Err(StoreError::InvalidInput(format!(
                    "{} is not a private SQLite sidecar",
                    sidecar.display()
                )));
            }
        }
    }
    Ok(())
}

fn secure_sidecars(path: &Path) -> StoreResult<()> {
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        if sidecar.exists() {
            fs::set_permissions(sidecar, fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(())
}

fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::now_v7().simple())
}

fn validate_identifier(label: &str, value: &str) -> StoreResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
    {
        return Err(StoreError::InvalidInput(format!(
            "{label} must be 1-128 lowercase ASCII identifier characters"
        )));
    }
    Ok(())
}

fn validate_text(label: &str, value: &str) -> StoreResult<()> {
    if value.is_empty() || value.len() > 4096 {
        return Err(StoreError::InvalidInput(format!(
            "{label} must contain 1-4096 UTF-8 bytes"
        )));
    }
    Ok(())
}

fn validate_digest(label: &str, value: &str) -> StoreResult<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(StoreError::InvalidInput(format!(
            "{label} must be a lowercase SHA-256 hex digest"
        )));
    }
    Ok(())
}

fn validate_new_basis(input: &NewFrozenBasis) -> StoreResult<()> {
    validate_text("basis label", &input.label)?;
    validate_text("basis verifier version", &input.verifier_version)?;
    match input.kind {
        BasisKind::Steward => {
            let scope_id = input.scope_id.as_deref().ok_or_else(|| {
                StoreError::InvalidInput("steward basis requires a scope id".into())
            })?;
            validate_identifier("basis scope id", scope_id)?;
            if input.scope_version.is_none_or(|version| version == 0) {
                return Err(StoreError::InvalidInput(
                    "steward basis requires a positive scope version".into(),
                ));
            }
        }
        BasisKind::Candidate => {
            if input.scope_id.is_some() || input.scope_version.is_some() {
                return Err(StoreError::InvalidInput(
                    "candidate basis must not impersonate a steward scope".into(),
                ));
            }
        }
    }
    if input.observed_at <= 0 {
        return Err(StoreError::InvalidInput(
            "basis observation time must be positive".into(),
        ));
    }
    if input.sources.is_empty() || input.sources.len() > MAX_FROZEN_SOURCES {
        return Err(StoreError::InvalidInput(format!(
            "a frozen basis requires 1-{MAX_FROZEN_SOURCES} sources"
        )));
    }
    let mut ids = BTreeSet::new();
    let mut total_bytes = 0_usize;
    for source in &input.sources {
        validate_frozen_source(source)?;
        total_bytes = total_bytes
            .checked_add(source.content.len())
            .ok_or_else(|| {
                StoreError::InvalidInput("frozen source byte total overflowed".into())
            })?;
        if total_bytes > MAX_FROZEN_CATALOG_BYTES {
            return Err(StoreError::InvalidInput(format!(
                "frozen basis sources exceed the {MAX_FROZEN_CATALOG_BYTES}-byte catalog limit"
            )));
        }
        if !ids.insert(source.source_id.as_str()) {
            return Err(StoreError::InvalidInput(format!(
                "frozen source id {} is duplicated",
                source.source_id
            )));
        }
    }
    Ok(())
}

fn validate_frozen_source(source: &NewFrozenSource) -> StoreResult<()> {
    validate_text("source id", &source.source_id)?;
    validate_text("source kind", &source.kind)?;
    validate_text("source locator", &source.locator)?;
    if let Some(origin_path) = &source.origin_path {
        validate_text("source origin path", origin_path)?;
    }
    if let Some(revision) = &source.revision {
        validate_text("source revision", revision)?;
    }
    if source.observed_at <= 0 {
        return Err(StoreError::InvalidInput(format!(
            "source {} observation time must be positive",
            source.source_id
        )));
    }
    if source.content.len() > MAX_FROZEN_SOURCE_BYTES {
        return Err(StoreError::InvalidInput(format!(
            "source {} exceeds the {}-byte limit",
            source.source_id, MAX_FROZEN_SOURCE_BYTES
        )));
    }
    std::str::from_utf8(&source.content).map_err(|_| {
        StoreError::InvalidInput(format!(
            "source {} must contain exact UTF-8 bytes",
            source.source_id
        ))
    })?;
    Ok(())
}

fn put_digest_field(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn put_optional_digest_field(hasher: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            hasher.update([1]);
            put_digest_field(hasher, value.as_bytes());
        }
        None => hasher.update([0]),
    }
}

fn decode_markdown(bytes: Vec<u8>) -> StoreResult<OpaqueMarkdown> {
    OpaqueMarkdown::new(bytes)
        .map_err(|error| StoreError::CorruptState(format!("invalid stored Markdown: {error}")))
}

fn decode_optional_markdown(bytes: Option<Vec<u8>>) -> StoreResult<Option<OpaqueMarkdown>> {
    bytes.map(decode_markdown).transpose()
}

fn sql_markdown(bytes: Vec<u8>) -> rusqlite::Result<OpaqueMarkdown> {
    OpaqueMarkdown::new(bytes).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(error))
    })
}

fn sql_enum_error(column: usize, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid stored enum value {value}"),
        )),
    )
}

fn collect_rows<T>(rows: impl IntoIterator<Item = rusqlite::Result<T>>) -> StoreResult<Vec<T>> {
    rows.into_iter()
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(StoreError::from)
}

fn decode_scope_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StewardScopeView> {
    Ok(StewardScopeView {
        scope_id: row.get(0)?,
        version: row.get(1)?,
        steward_party: row.get(2)?,
        title: row.get(3)?,
        charter_markdown: sql_markdown(row.get(4)?)?,
        charter_sha256: row.get(5)?,
        descriptor_sha256: row.get(6)?,
        recorded_at: row.get(7)?,
    })
}

fn read_scope_optional(
    connection: &Connection,
    scope_id: &str,
    version: u32,
) -> StoreResult<Option<StewardScopeView>> {
    Ok(connection
        .query_row(
            "SELECT scope_id, version, steward_party, title, charter_markdown,
                    charter_sha256, descriptor_sha256, recorded_at
             FROM steward_scopes WHERE scope_id = ? AND version = ?",
            params![scope_id, version],
            decode_scope_row,
        )
        .optional()?)
}

fn read_scope(
    connection: &Connection,
    scope_id: &str,
    version: u32,
) -> StoreResult<StewardScopeView> {
    read_scope_optional(connection, scope_id, version)?.ok_or_else(|| {
        StoreError::NotFound(format!(
            "steward scope {scope_id} version {version} does not exist"
        ))
    })
}

fn insert_frozen_basis(
    transaction: &Transaction<'_>,
    basis_id: &str,
    input: &NewFrozenBasis,
    manifest_sha256: &str,
    recorded_at: i64,
) -> StoreResult<()> {
    transaction.execute(
        "INSERT INTO frozen_bases (
            basis_id, basis_kind, label, scope_id, scope_version,
            verifier_version, manifest_sha256, observed_at, recorded_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            basis_id,
            input.kind.as_str(),
            input.label,
            input.scope_id,
            input.scope_version,
            input.verifier_version,
            manifest_sha256,
            input.observed_at,
            recorded_at,
        ],
    )?;
    for (ordinal, source) in input.sources.iter().enumerate() {
        transaction.execute(
            "INSERT INTO frozen_basis_sources (
                basis_id, ordinal, source_id, kind, locator, origin_path,
                revision, content, content_sha256, observed_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                basis_id,
                i64::try_from(ordinal)
                    .map_err(|_| StoreError::CorruptState("source ordinal overflow".into()))?,
                source.source_id,
                source.kind,
                source.locator,
                source.origin_path,
                source.revision,
                source.content,
                sha256_hex(&source.content),
                source.observed_at,
            ],
        )?;
    }
    Ok(())
}

fn read_basis(connection: &Connection, basis_id: &str) -> StoreResult<FrozenBasisView> {
    let row = connection.query_row(
        "SELECT basis_id, basis_kind, label, scope_id, scope_version,
                verifier_version, manifest_sha256, observed_at, recorded_at
         FROM frozen_bases WHERE basis_id = ?",
        [basis_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<u32>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
            ))
        },
    );
    let (
        id,
        kind,
        label,
        scope_id,
        scope_version,
        verifier_version,
        manifest,
        observed_at,
        recorded_at,
    ) = match row {
        Ok(value) => value,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(StoreError::NotFound(format!(
                "frozen basis {basis_id} does not exist"
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let kind = BasisKind::parse(&kind)
        .ok_or_else(|| StoreError::CorruptState("invalid frozen basis kind".into()))?;
    let mut statement = connection.prepare(
        "SELECT ordinal, source_id, kind, locator, origin_path, revision,
                content, content_sha256, observed_at
         FROM frozen_basis_sources WHERE basis_id = ? ORDER BY ordinal",
    )?;
    let rows = statement.query_map([basis_id], |row| {
        Ok(FrozenSourceView {
            ordinal: row.get(0)?,
            source_id: row.get(1)?,
            kind: row.get(2)?,
            locator: row.get(3)?,
            origin_path: row.get(4)?,
            revision: row.get(5)?,
            content: row.get(6)?,
            content_sha256: row.get(7)?,
            observed_at: row.get(8)?,
        })
    })?;
    Ok(FrozenBasisView {
        basis_id: id,
        kind,
        label,
        scope_id,
        scope_version,
        verifier_version,
        manifest_sha256: manifest,
        observed_at,
        recorded_at,
        sources: collect_rows(rows)?,
        freshness: basis_freshness(connection, basis_id)?,
    })
}

fn basis_freshness(connection: &Connection, basis_id: &str) -> StoreResult<BasisFreshness> {
    let value: Option<String> = connection
        .query_row(
            "SELECT outcome FROM basis_verifications
             WHERE basis_id = ? ORDER BY checked_at DESC, verification_id DESC LIMIT 1",
            [basis_id],
            |row| row.get(0),
        )
        .optional()?;
    value.map_or(Ok(BasisFreshness::Unknown), |value| {
        BasisFreshness::parse(&value)
            .ok_or_else(|| StoreError::CorruptState("invalid basis freshness".into()))
    })
}

fn basis_applicability_fingerprint(
    connection: &Connection,
    basis_id: &str,
) -> StoreResult<(BasisFreshness, Option<String>)> {
    let row: Option<(String, Option<String>)> = connection
        .query_row(
            "SELECT outcome, observed_manifest_sha256 FROM basis_verifications
             WHERE basis_id = ? ORDER BY checked_at DESC, verification_id DESC LIMIT 1",
            [basis_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    row.map_or(
        Ok((BasisFreshness::Unknown, None)),
        |(outcome, observed)| {
            let outcome = BasisFreshness::parse(&outcome)
                .ok_or_else(|| StoreError::CorruptState("invalid basis freshness".into()))?;
            Ok((outcome, observed))
        },
    )
}

fn insert_basis_verification(
    transaction: &Transaction<'_>,
    basis_id: &str,
    observed_manifest_sha256: Option<&str>,
    detail_markdown: Option<&OpaqueMarkdown>,
) -> StoreResult<BasisVerificationView> {
    let basis = read_basis(transaction, basis_id)?;
    let outcome = match observed_manifest_sha256 {
        Some(observed) if observed == basis.manifest_sha256 => BasisFreshness::Fresh,
        Some(_) => BasisFreshness::Stale,
        None => BasisFreshness::Unknown,
    };
    let verification_id = new_id("ver");
    let checked_at = now_unix();
    transaction.execute(
        "INSERT INTO basis_verifications (
            verification_id, basis_id, outcome, observed_manifest_sha256,
            detail_markdown, checked_at
         ) VALUES (?, ?, ?, ?, ?, ?)",
        params![
            verification_id,
            basis_id,
            outcome.as_str(),
            observed_manifest_sha256,
            detail_markdown.map(OpaqueMarkdown::as_bytes),
            checked_at,
        ],
    )?;
    Ok(BasisVerificationView {
        verification_id,
        basis_id: basis_id.into(),
        outcome,
        observed_manifest_sha256: observed_manifest_sha256.map(str::to_owned),
        detail_markdown: detail_markdown.cloned(),
        checked_at,
    })
}

fn require_basis_digest(basis: &FrozenBasisView, observed: &str) -> StoreResult<()> {
    validate_digest("observed basis digest", observed)?;
    if basis.manifest_sha256 != observed {
        return Err(StoreError::Stale(format!(
            "basis {} no longer matches its frozen manifest",
            basis.basis_id
        )));
    }
    Ok(())
}

fn require_integration(connection: &Connection, integration_id: &str) -> StoreResult<()> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM integrations WHERE integration_id = ?)",
        [integration_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(StoreError::NotFound(format!(
            "integration {integration_id} does not exist"
        )));
    }
    Ok(())
}

fn next_integration_ordinal(
    transaction: &Transaction<'_>,
    integration_id: &str,
) -> StoreResult<u32> {
    let current: u32 = transaction.query_row(
        "SELECT COALESCE(MAX(ordinal), 0) FROM integration_events WHERE integration_id = ?",
        [integration_id],
        |row| row.get(0),
    )?;
    current
        .checked_add(1)
        .ok_or_else(|| StoreError::CorruptState("integration event ordinal overflow".into()))
}

fn decode_track_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TrackView> {
    Ok(TrackView {
        track_id: row.get(0)?,
        integration_id: row.get(1)?,
        scope_id: row.get(2)?,
        scope_version: row.get(3)?,
        steward_party: row.get(4)?,
        created_at: row.get(5)?,
        active: row.get(6)?,
    })
}

fn read_track(connection: &Connection, track_id: &str) -> StoreResult<TrackView> {
    let result = connection.query_row(
        "SELECT t.track_id, t.integration_id, t.scope_id, t.scope_version,
                t.steward_party, t.created_at,
                NOT EXISTS (
                  SELECT 1 FROM integration_events e
                  WHERE e.integration_id = t.integration_id
                    AND e.track_id = t.track_id AND e.kind = 'track_retired'
                ) AS active
         FROM integration_tracks t WHERE t.track_id = ?",
        [track_id],
        decode_track_row,
    );
    match result {
        Ok(value) => Ok(value),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(StoreError::NotFound(format!(
            "track {track_id} does not exist"
        ))),
        Err(error) => Err(error.into()),
    }
}

fn roster_digest(integration_id: &str, revision: u32, tracks: &[TrackView]) -> String {
    let mut hasher = Sha256::new();
    put_digest_field(&mut hasher, b"pratica-roster-v1");
    put_digest_field(&mut hasher, integration_id.as_bytes());
    hasher.update(revision.to_be_bytes());
    for track in tracks.iter().filter(|track| track.active) {
        put_digest_field(&mut hasher, track.track_id.as_bytes());
        put_digest_field(&mut hasher, track.scope_id.as_bytes());
        hasher.update(track.scope_version.to_be_bytes());
        put_digest_field(&mut hasher, track.steward_party.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn require_track_available(transaction: &Transaction<'_>, track: &TrackView) -> StoreResult<()> {
    if !track.active {
        return Err(StoreError::Conflict(format!(
            "track {} is retired",
            track.track_id
        )));
    }
    if open_negotiation_id(transaction, &track.track_id)?.is_some() {
        return Err(StoreError::Conflict(format!(
            "track {} already has an open negotiation",
            track.track_id
        )));
    }
    Ok(())
}

fn open_negotiation_id(connection: &Connection, track_id: &str) -> StoreResult<Option<String>> {
    Ok(connection
        .query_row(
            "SELECT n.negotiation_id
             FROM negotiations n
             WHERE n.track_id = ?
               AND NOT EXISTS (
                 SELECT 1 FROM agreements a WHERE a.negotiation_id = n.negotiation_id
               )
               AND NOT EXISTS (
                 SELECT 1 FROM negotiation_events e
                 WHERE e.negotiation_id = n.negotiation_id AND e.kind = 'cancelled'
               )
             ORDER BY n.created_at DESC, n.negotiation_id DESC LIMIT 1",
            [track_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn latest_negotiation_for_track(
    connection: &Connection,
    track_id: &str,
) -> StoreResult<Option<String>> {
    Ok(connection
        .query_row(
            "SELECT negotiation_id FROM negotiations
             WHERE track_id = ? ORDER BY created_at DESC, negotiation_id DESC LIMIT 1",
            [track_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn insert_negotiation(
    transaction: &Transaction<'_>,
    negotiation_id: &str,
    track_id: &str,
    kind: NegotiationKind,
    predecessor_agreement_id: Option<&str>,
    recorded_at: i64,
) -> StoreResult<()> {
    transaction.execute(
        "INSERT INTO negotiations (
            negotiation_id, track_id, kind, predecessor_agreement_id, created_at
         ) VALUES (?, ?, ?, ?, ?)",
        params![
            negotiation_id,
            track_id,
            kind.as_str(),
            predecessor_agreement_id,
            recorded_at,
        ],
    )?;
    append_negotiation_event(
        transaction,
        negotiation_id,
        NegotiationEventKind::Opened,
        None,
        None,
        None,
        None,
        None,
        None,
        recorded_at,
    )?;
    Ok(())
}

fn insert_offer_event(
    transaction: &Transaction<'_>,
    offer_id: &str,
    negotiation_id: &str,
    author_role: PartyRole,
    terms: &OpaqueMarkdown,
    basis_id: Option<&str>,
    recorded_at: i64,
) -> StoreResult<i64> {
    insert_offer_event_with_review(
        transaction,
        offer_id,
        negotiation_id,
        author_role,
        terms,
        basis_id,
        None,
        None,
        recorded_at,
    )
}

#[allow(clippy::too_many_arguments)]
fn insert_offer_event_with_review(
    transaction: &Transaction<'_>,
    offer_id: &str,
    negotiation_id: &str,
    author_role: PartyRole,
    terms: &OpaqueMarkdown,
    basis_id: Option<&str>,
    review_markdown: Option<&OpaqueMarkdown>,
    attempt_id: Option<&str>,
    recorded_at: i64,
) -> StoreResult<i64> {
    if author_role == PartyRole::Steward && basis_id.is_none() {
        return Err(StoreError::InvalidInput(
            "a steward proposal requires a frozen basis".into(),
        ));
    }
    transaction.execute(
        "INSERT INTO offers (
            offer_id, negotiation_id, author_role, terms_markdown,
            terms_sha256, basis_id, recorded_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![
            offer_id,
            negotiation_id,
            author_role.as_str(),
            terms.as_bytes(),
            terms.sha256(),
            basis_id,
            recorded_at,
        ],
    )?;
    append_negotiation_event(
        transaction,
        negotiation_id,
        NegotiationEventKind::OfferSubmitted,
        Some(author_role),
        Some(offer_id),
        basis_id,
        review_markdown,
        None,
        attempt_id,
        recorded_at,
    )
}

#[allow(clippy::too_many_arguments)]
fn append_negotiation_event(
    transaction: &Transaction<'_>,
    negotiation_id: &str,
    kind: NegotiationEventKind,
    party_role: Option<PartyRole>,
    offer_id: Option<&str>,
    basis_id: Option<&str>,
    review_markdown: Option<&OpaqueMarkdown>,
    reason: Option<&str>,
    attempt_id: Option<&str>,
    recorded_at: i64,
) -> StoreResult<i64> {
    let ordinal: u32 = transaction.query_row(
        "SELECT COALESCE(MAX(ordinal), 0) + 1
         FROM negotiation_events WHERE negotiation_id = ?",
        [negotiation_id],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO negotiation_events (
            negotiation_id, ordinal, kind, party_role, offer_id, basis_id,
            review_markdown, reason, attempt_id, recorded_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            negotiation_id,
            ordinal,
            kind.as_str(),
            party_role.map(PartyRole::as_str),
            offer_id,
            basis_id,
            review_markdown.map(OpaqueMarkdown::as_bytes),
            reason,
            attempt_id,
            recorded_at,
        ],
    )?;
    Ok(transaction.last_insert_rowid())
}

fn require_negotiation(connection: &Connection, negotiation_id: &str) -> StoreResult<()> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM negotiations WHERE negotiation_id = ?)",
        [negotiation_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(StoreError::NotFound(format!(
            "negotiation {negotiation_id} does not exist"
        )));
    }
    Ok(())
}

fn require_open_negotiation(
    connection: &Connection,
    negotiation_id: &str,
) -> StoreResult<TrackView> {
    require_negotiation(connection, negotiation_id)?;
    let sealed: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM agreements WHERE negotiation_id = ?)",
        [negotiation_id],
        |row| row.get(0),
    )?;
    if sealed {
        return Err(StoreError::Conflict(format!(
            "negotiation {negotiation_id} is sealed"
        )));
    }
    let cancelled: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM negotiation_events
            WHERE negotiation_id = ? AND kind = 'cancelled'
         )",
        [negotiation_id],
        |row| row.get(0),
    )?;
    if cancelled {
        return Err(StoreError::Conflict(format!(
            "negotiation {negotiation_id} is cancelled"
        )));
    }
    let track_id: String = connection.query_row(
        "SELECT track_id FROM negotiations WHERE negotiation_id = ?",
        [negotiation_id],
        |row| row.get(0),
    )?;
    let track = read_track(connection, &track_id)?;
    if !track.active {
        return Err(StoreError::Conflict(format!(
            "negotiation track {track_id} is retired"
        )));
    }
    Ok(track)
}

fn current_head_id(connection: &Connection, negotiation_id: &str) -> StoreResult<Option<String>> {
    Ok(connection
        .query_row(
            "SELECT offer_id FROM negotiation_events
             WHERE negotiation_id = ? AND kind = 'offer_submitted'
             ORDER BY ordinal DESC LIMIT 1",
            [negotiation_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn require_expected_head(
    connection: &Connection,
    negotiation_id: &str,
    expected_head_id: &str,
) -> StoreResult<()> {
    let current = current_head_id(connection, negotiation_id)?;
    if current.as_deref() != Some(expected_head_id) {
        return Err(StoreError::Stale(format!(
            "negotiation {negotiation_id} head changed"
        )));
    }
    Ok(())
}

#[derive(Debug)]
struct EffectiveAssent {
    kind: NegotiationEventKind,
    offer_id: Option<String>,
    basis_id: Option<String>,
    ordinal: u32,
}

fn effective_assent(
    connection: &Connection,
    negotiation_id: &str,
    role: PartyRole,
) -> StoreResult<Option<EffectiveAssent>> {
    let row = connection
        .query_row(
            "SELECT kind, offer_id, basis_id, ordinal
             FROM negotiation_events
             WHERE negotiation_id = ? AND party_role = ?
               AND kind IN (
                    'offer_submitted', 'assent', 'assent_withdrawn', 'steward_blocked'
               )
             ORDER BY ordinal DESC LIMIT 1",
            params![negotiation_id, role.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            },
        )
        .optional()?;
    row.map(|(kind, offer_id, basis_id, ordinal)| {
        let kind = NegotiationEventKind::parse(&kind)
            .ok_or_else(|| StoreError::CorruptState("invalid assent event kind".into()))?;
        Ok(EffectiveAssent {
            kind,
            offer_id,
            basis_id,
            ordinal,
        })
    })
    .transpose()
}

fn require_steward_basis(
    connection: &Connection,
    track: &TrackView,
    guard: &BasisGuard,
) -> StoreResult<FrozenBasisView> {
    let basis = read_basis(connection, &guard.basis_id)?;
    if basis.kind != BasisKind::Steward
        || basis.scope_id.as_deref() != Some(track.scope_id.as_str())
        || basis.scope_version != Some(track.scope_version)
    {
        return Err(StoreError::Conflict(format!(
            "basis {} does not belong to track {}",
            basis.basis_id, track.track_id
        )));
    }
    require_basis_digest(&basis, &guard.observed_manifest_sha256)?;
    Ok(basis)
}

fn maybe_seal(
    transaction: &Transaction<'_>,
    negotiation_id: &str,
    track: &TrackView,
    guard: Option<&BasisGuard>,
    recorded_at: i64,
) -> StoreResult<Option<String>> {
    let head = current_head_id(transaction, negotiation_id)?
        .ok_or_else(|| StoreError::CorruptState("an open negotiation has no terms head".into()))?;
    let entrant = effective_assent(transaction, negotiation_id, PartyRole::Entrant)?;
    let steward = effective_assent(transaction, negotiation_id, PartyRole::Steward)?;
    let entrant_current = entrant.as_ref().is_some_and(|record| {
        matches!(
            record.kind,
            NegotiationEventKind::OfferSubmitted | NegotiationEventKind::Assent
        ) && record.offer_id.as_deref() == Some(head.as_str())
    });
    let steward_current = steward.as_ref().is_some_and(|record| {
        matches!(
            record.kind,
            NegotiationEventKind::OfferSubmitted | NegotiationEventKind::Assent
        ) && record.offer_id.as_deref() == Some(head.as_str())
    });
    if !entrant_current || !steward_current {
        return Ok(None);
    }
    let (kind, predecessor_id): (String, Option<String>) = transaction.query_row(
        "SELECT kind, predecessor_agreement_id FROM negotiations WHERE negotiation_id = ?",
        [negotiation_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let kind = NegotiationKind::parse(&kind)
        .ok_or_else(|| StoreError::CorruptState("invalid negotiation kind".into()))?;
    if kind == NegotiationKind::Amendment {
        let predecessor_id = predecessor_id.ok_or_else(|| {
            StoreError::CorruptState("amendment negotiation has no predecessor".into())
        })?;
        let current = active_agreement_for_track(transaction, &track.track_id)?
            .ok_or_else(|| StoreError::CorruptState("amendment track has no agreement".into()))?;
        let successor_exists: bool = transaction.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM negotiations n
                JOIN agreements a ON a.negotiation_id = n.negotiation_id
                WHERE n.predecessor_agreement_id = ?
             )",
            [&predecessor_id],
            |row| row.get(0),
        )?;
        if current.agreement_id != predecessor_id || successor_exists {
            return Err(StoreError::Stale(
                "amendment predecessor is no longer the active agreement".into(),
            ));
        }
    }
    let steward_basis_id = steward
        .and_then(|record| record.basis_id)
        .ok_or_else(|| StoreError::CorruptState("steward assent has no basis".into()))?;
    let guard = guard.ok_or_else(|| {
        StoreError::Stale("final assent requires a freshly observed steward basis".into())
    })?;
    if guard.basis_id != steward_basis_id {
        return Err(StoreError::Stale(
            "final assent did not verify the steward assent basis".into(),
        ));
    }
    require_steward_basis(transaction, track, guard)?;
    insert_basis_verification(
        transaction,
        &guard.basis_id,
        Some(&guard.observed_manifest_sha256),
        None,
    )?;
    let seal_event_id = append_negotiation_event(
        transaction,
        negotiation_id,
        NegotiationEventKind::AgreementSealed,
        None,
        Some(&head),
        Some(&guard.basis_id),
        None,
        None,
        None,
        recorded_at,
    )?;
    let agreement_id = new_id("agr");
    transaction.execute(
        "INSERT INTO agreements (
            agreement_id, negotiation_id, offer_id, basis_id,
            sealed_event_id, sealed_at
         ) VALUES (?, ?, ?, ?, ?, ?)",
        params![
            agreement_id,
            negotiation_id,
            head,
            guard.basis_id,
            seal_event_id,
            recorded_at,
        ],
    )?;
    Ok(Some(agreement_id))
}

fn read_offer(connection: &Connection, offer_id: &str) -> StoreResult<OfferView> {
    let row = connection.query_row(
        "SELECT offer_id, negotiation_id, author_role, terms_markdown,
                terms_sha256, basis_id, recorded_at
         FROM offers WHERE offer_id = ?",
        [offer_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
            ))
        },
    );
    match row {
        Ok((id, negotiation_id, author, terms, digest, basis_id, recorded_at)) => Ok(OfferView {
            offer_id: id,
            negotiation_id,
            author_role: PartyRole::parse(&author)
                .ok_or_else(|| StoreError::CorruptState("invalid offer author role".into()))?,
            terms_markdown: decode_markdown(terms)?,
            terms_sha256: digest,
            basis_id,
            recorded_at,
        }),
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(StoreError::NotFound(format!(
            "offer {offer_id} does not exist"
        ))),
        Err(error) => Err(error.into()),
    }
}

fn read_agreement(connection: &Connection, agreement_id: &str) -> StoreResult<AgreementView> {
    let row = connection.query_row(
        "SELECT a.agreement_id, a.negotiation_id, n.track_id, a.offer_id,
                a.basis_id, a.sealed_at, i.entrant_party, t.steward_party,
                n.predecessor_agreement_id
         FROM agreements a
         JOIN negotiations n ON n.negotiation_id = a.negotiation_id
         JOIN integration_tracks t ON t.track_id = n.track_id
         JOIN integrations i ON i.integration_id = t.integration_id
         WHERE a.agreement_id = ?",
        [agreement_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
            ))
        },
    );
    match row {
        Ok((
            id,
            negotiation_id,
            track_id,
            offer_id,
            basis_id,
            sealed_at,
            entrant_party,
            steward_party,
            predecessor_agreement_id,
        )) => {
            let entrant_assent = effective_assent(connection, &negotiation_id, PartyRole::Entrant)?
                .ok_or_else(|| {
                    StoreError::CorruptState("sealed agreement lacks entrant assent".into())
                })?;
            let steward_assent = effective_assent(connection, &negotiation_id, PartyRole::Steward)?
                .ok_or_else(|| {
                    StoreError::CorruptState("sealed agreement lacks steward assent".into())
                })?;
            let successor_agreement_id = connection
                .query_row(
                    "SELECT a.agreement_id
                     FROM negotiations n JOIN agreements a ON a.negotiation_id = n.negotiation_id
                     WHERE n.predecessor_agreement_id = ?
                     ORDER BY a.sealed_at, a.agreement_id LIMIT 1",
                    [&id],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(AgreementView {
                agreement_id: id,
                negotiation_id,
                track_id,
                entrant_party,
                steward_party,
                offer: read_offer(connection, &offer_id)?,
                basis_freshness: basis_freshness(connection, &basis_id)?,
                basis_id,
                entrant_assent_event_ordinal: entrant_assent.ordinal,
                steward_assent_event_ordinal: steward_assent.ordinal,
                predecessor_agreement_id,
                successor_agreement_id,
                sealed_at,
            })
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(StoreError::NotFound(format!(
            "agreement {agreement_id} does not exist"
        ))),
        Err(error) => Err(error.into()),
    }
}

#[allow(clippy::too_many_lines)]
fn verify_agreement_integrity(
    connection: &Connection,
    agreement_id: &str,
) -> StoreResult<AgreementView> {
    let agreement = read_agreement(connection, agreement_id)?;
    let corrupt = |detail: &str| {
        StoreError::CorruptState(format!(
            "agreement {agreement_id} integrity check failed: {detail}"
        ))
    };
    if sha256_hex(agreement.offer.terms_markdown.as_bytes()) != agreement.offer.terms_sha256 {
        return Err(corrupt("offer terms digest does not match its exact bytes"));
    }
    if agreement.offer.negotiation_id != agreement.negotiation_id {
        return Err(corrupt("offer belongs to another negotiation"));
    }

    let basis = read_basis(connection, &agreement.basis_id)?;
    for source in &basis.sources {
        if sha256_hex(&source.content) != source.content_sha256 {
            return Err(corrupt(
                "frozen source digest does not match its exact bytes",
            ));
        }
    }
    let basis_input = NewFrozenBasis {
        kind: basis.kind,
        label: basis.label.clone(),
        scope_id: basis.scope_id.clone(),
        scope_version: basis.scope_version,
        verifier_version: basis.verifier_version.clone(),
        observed_at: basis.observed_at,
        sources: basis
            .sources
            .iter()
            .map(|source| NewFrozenSource {
                source_id: source.source_id.clone(),
                kind: source.kind.clone(),
                locator: source.locator.clone(),
                origin_path: source.origin_path.clone(),
                revision: source.revision.clone(),
                content: source.content.clone(),
                observed_at: source.observed_at,
            })
            .collect(),
    };
    if Store::basis_manifest_sha256(&basis_input)? != basis.manifest_sha256 {
        return Err(corrupt("frozen basis manifest digest is invalid"));
    }

    let track = read_track(connection, &agreement.track_id)?;
    if basis.kind != BasisKind::Steward
        || basis.scope_id.as_deref() != Some(track.scope_id.as_str())
        || basis.scope_version != Some(track.scope_version)
    {
        return Err(corrupt(
            "frozen basis does not belong to the agreement track",
        ));
    }
    match agreement.offer.author_role {
        PartyRole::Entrant if agreement.offer.basis_id.is_some() => {
            return Err(corrupt(
                "entrant offer unexpectedly carries a steward basis",
            ));
        }
        PartyRole::Steward
            if agreement.offer.basis_id.as_deref() != Some(agreement.basis_id.as_str()) =>
        {
            return Err(corrupt("steward offer carries a different basis"));
        }
        PartyRole::Entrant | PartyRole::Steward => {}
    }

    let (kind, predecessor_id, sealed_event_id, sealed_at): (String, Option<String>, i64, i64) =
        connection.query_row(
            "SELECT n.kind, n.predecessor_agreement_id, a.sealed_event_id, a.sealed_at
             FROM agreements a JOIN negotiations n ON n.negotiation_id = a.negotiation_id
             WHERE a.agreement_id = ?",
            [agreement_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
    let kind =
        NegotiationKind::parse(&kind).ok_or_else(|| corrupt("negotiation kind is invalid"))?;
    match (kind, predecessor_id.as_deref()) {
        (NegotiationKind::Initial, None) => {}
        (NegotiationKind::Amendment, Some(predecessor_id)) => {
            if predecessor_id == agreement_id {
                return Err(corrupt("amendment references itself as predecessor"));
            }
            let predecessor = read_agreement(connection, predecessor_id)?;
            if predecessor.track_id != agreement.track_id || predecessor.sealed_at > sealed_at {
                return Err(corrupt(
                    "amendment predecessor is not an earlier track agreement",
                ));
            }
            let successor_count: u32 = connection.query_row(
                "SELECT COUNT(*) FROM negotiations n
                 JOIN agreements a ON a.negotiation_id = n.negotiation_id
                 WHERE n.predecessor_agreement_id = ?",
                [predecessor_id],
                |row| row.get(0),
            )?;
            if successor_count != 1 {
                return Err(corrupt("predecessor has multiple sealed successors"));
            }
        }
        (NegotiationKind::Initial, Some(_)) | (NegotiationKind::Amendment, None) => {
            return Err(corrupt("negotiation kind and predecessor disagree"));
        }
    }

    let (event_negotiation, event_kind, event_offer, event_basis, event_at): (
        String,
        String,
        Option<String>,
        Option<String>,
        i64,
    ) = connection.query_row(
        "SELECT negotiation_id, kind, offer_id, basis_id, recorded_at
         FROM negotiation_events WHERE event_id = ?",
        [sealed_event_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )?;
    if event_negotiation != agreement.negotiation_id
        || event_kind != NegotiationEventKind::AgreementSealed.as_str()
        || event_offer.as_deref() != Some(agreement.offer.offer_id.as_str())
        || event_basis.as_deref() != Some(agreement.basis_id.as_str())
        || event_at != sealed_at
    {
        return Err(corrupt("seal event does not exactly bind the agreement"));
    }
    if current_head_id(connection, &agreement.negotiation_id)?.as_deref()
        != Some(agreement.offer.offer_id.as_str())
    {
        return Err(corrupt("sealed offer is not the negotiation head"));
    }
    let entrant = effective_assent(connection, &agreement.negotiation_id, PartyRole::Entrant)?;
    let steward = effective_assent(connection, &agreement.negotiation_id, PartyRole::Steward)?;
    let accepted = |record: &EffectiveAssent| {
        matches!(
            record.kind,
            NegotiationEventKind::OfferSubmitted | NegotiationEventKind::Assent
        ) && record.offer_id.as_deref() == Some(agreement.offer.offer_id.as_str())
    };
    if !entrant.as_ref().is_some_and(accepted)
        || !steward.as_ref().is_some_and(|record| {
            accepted(record) && record.basis_id.as_deref() == Some(agreement.basis_id.as_str())
        })
    {
        return Err(corrupt(
            "current bilateral assent evidence does not match the seal",
        ));
    }
    if entrant.as_ref().map(|record| record.ordinal) != Some(agreement.entrant_assent_event_ordinal)
        || steward.as_ref().map(|record| record.ordinal)
            != Some(agreement.steward_assent_event_ordinal)
    {
        return Err(corrupt("projected assent evidence is inconsistent"));
    }
    if let Some(successor_id) = &agreement.successor_agreement_id {
        let successor = read_agreement(connection, successor_id)?;
        if successor.track_id != agreement.track_id
            || successor.predecessor_agreement_id.as_deref() != Some(agreement_id)
        {
            return Err(corrupt("successor does not form a valid amendment link"));
        }
    }
    Ok(agreement)
}

fn active_agreement_for_track(
    connection: &Connection,
    track_id: &str,
) -> StoreResult<Option<AgreementView>> {
    let agreement_id: Option<String> = connection
        .query_row(
            "SELECT a.agreement_id
             FROM agreements a
             JOIN negotiations n ON n.negotiation_id = a.negotiation_id
             WHERE n.track_id = ?
             ORDER BY a.sealed_at DESC, a.agreement_id DESC LIMIT 1",
            [track_id],
            |row| row.get(0),
        )
        .optional()?;
    agreement_id
        .map(|id| read_agreement(connection, &id))
        .transpose()
}

fn read_negotiation(connection: &Connection, negotiation_id: &str) -> StoreResult<NegotiationView> {
    let row = connection.query_row(
        "SELECT negotiation_id, track_id, kind, predecessor_agreement_id, created_at
         FROM negotiations WHERE negotiation_id = ?",
        [negotiation_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i64>(4)?,
            ))
        },
    );
    let (id, track_id, kind, predecessor, created_at) = match row {
        Ok(value) => value,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(StoreError::NotFound(format!(
                "negotiation {negotiation_id} does not exist"
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let track = read_track(connection, &track_id)?;
    let entrant_party: String = connection.query_row(
        "SELECT i.entrant_party
         FROM integrations i JOIN integration_tracks t ON t.integration_id = i.integration_id
         WHERE t.track_id = ?",
        [&track_id],
        |row| row.get(0),
    )?;
    let head = current_head_id(connection, negotiation_id)?
        .map(|offer_id| read_offer(connection, &offer_id))
        .transpose()?;
    let agreement_id: Option<String> = connection
        .query_row(
            "SELECT agreement_id FROM agreements WHERE negotiation_id = ?",
            [negotiation_id],
            |row| row.get(0),
        )
        .optional()?;
    let agreement = agreement_id
        .map(|agreement_id| read_agreement(connection, &agreement_id))
        .transpose()?;
    let cancelled: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM negotiation_events
            WHERE negotiation_id = ? AND kind = 'cancelled'
         )",
        [negotiation_id],
        |row| row.get(0),
    )?;
    let status = if agreement.is_some() {
        NegotiationStatus::Sealed
    } else if cancelled {
        NegotiationStatus::Cancelled
    } else {
        NegotiationStatus::Open
    };
    let head_id = head.as_ref().map(|offer| offer.offer_id.as_str());
    Ok(NegotiationView {
        negotiation_id: id,
        track_id,
        kind: NegotiationKind::parse(&kind)
            .ok_or_else(|| StoreError::CorruptState("invalid negotiation kind".into()))?,
        predecessor_agreement_id: predecessor,
        status,
        entrant: assent_projection(
            connection,
            negotiation_id,
            PartyRole::Entrant,
            entrant_party,
            head_id,
        )?,
        steward: assent_projection(
            connection,
            negotiation_id,
            PartyRole::Steward,
            track.steward_party,
            head_id,
        )?,
        head,
        agreement,
        created_at,
    })
}

fn assent_projection(
    connection: &Connection,
    negotiation_id: &str,
    role: PartyRole,
    party: String,
    head_id: Option<&str>,
) -> StoreResult<PartyAssentView> {
    let record = effective_assent(connection, negotiation_id, role)?;
    let status = match &record {
        None => AssentStatus::None,
        Some(record) if record.kind == NegotiationEventKind::AssentWithdrawn => {
            AssentStatus::Withdrawn
        }
        Some(record) if record.offer_id.as_deref() != head_id => AssentStatus::StaleTerms,
        Some(record) if record.kind == NegotiationEventKind::StewardBlocked => {
            AssentStatus::Blocked
        }
        Some(record) if role == PartyRole::Steward => {
            let freshness = record
                .basis_id
                .as_deref()
                .map(|basis_id| basis_freshness(connection, basis_id))
                .transpose()?
                .unwrap_or(BasisFreshness::Unknown);
            match freshness {
                BasisFreshness::Fresh => AssentStatus::Current,
                BasisFreshness::Stale => AssentStatus::StaleBasis,
                BasisFreshness::Unknown => AssentStatus::UnknownBasis,
            }
        }
        Some(_) => AssentStatus::Current,
    };
    Ok(PartyAssentView {
        role,
        party,
        status,
        offer_id: record.as_ref().and_then(|record| record.offer_id.clone()),
        basis_id: record.as_ref().and_then(|record| record.basis_id.clone()),
        event_ordinal: record.map(|record| record.ordinal),
    })
}

fn decode_negotiation_event_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NegotiationEventView> {
    let kind: String = row.get(3)?;
    let role: Option<String> = row.get(4)?;
    let review: Option<Vec<u8>> = row.get(7)?;
    Ok(NegotiationEventView {
        event_id: row.get(0)?,
        negotiation_id: row.get(1)?,
        ordinal: row.get(2)?,
        kind: NegotiationEventKind::parse(&kind).ok_or_else(|| sql_enum_error(3, &kind))?,
        party_role: role
            .map(|value| PartyRole::parse(&value).ok_or_else(|| sql_enum_error(4, &value)))
            .transpose()?,
        offer_id: row.get(5)?,
        basis_id: row.get(6)?,
        review_markdown: review.map(sql_markdown).transpose()?,
        reason: row.get(8)?,
        attempt_id: row.get(9)?,
        recorded_at: row.get(10)?,
    })
}

fn composition_basis(
    connection: &Connection,
    integration_id: &str,
) -> StoreResult<(RosterView, Vec<CompositionAgreementRef>, String)> {
    require_integration(connection, integration_id)?;
    let revision: u32 = connection.query_row(
        "SELECT COALESCE(MAX(ordinal), 0) FROM integration_events WHERE integration_id = ?",
        [integration_id],
        |row| row.get(0),
    )?;
    let mut statement = connection.prepare(
        "SELECT t.track_id, t.integration_id, t.scope_id, t.scope_version,
                t.steward_party, t.created_at,
                NOT EXISTS (
                  SELECT 1 FROM integration_events e
                  WHERE e.integration_id = t.integration_id
                    AND e.track_id = t.track_id AND e.kind = 'track_retired'
                ) AS active
         FROM integration_tracks t WHERE t.integration_id = ? ORDER BY t.track_id",
    )?;
    let tracks = collect_rows(statement.query_map([integration_id], decode_track_row)?)?;
    let roster = RosterView {
        integration_id: integration_id.into(),
        revision,
        digest: roster_digest(integration_id, revision, &tracks),
        tracks,
    };
    let mut references = Vec::new();
    let mut hasher = Sha256::new();
    put_digest_field(&mut hasher, b"pratica-composition-basis-v1");
    put_digest_field(&mut hasher, roster.digest.as_bytes());
    for track in roster.tracks.iter().filter(|track| track.active) {
        put_digest_field(&mut hasher, track.track_id.as_bytes());
        if let Some(agreement) = active_agreement_for_track(connection, &track.track_id)? {
            hasher.update([1]);
            put_digest_field(&mut hasher, agreement.agreement_id.as_bytes());
            put_digest_field(&mut hasher, agreement.offer.terms_sha256.as_bytes());
            put_digest_field(&mut hasher, agreement.basis_id.as_bytes());
            let (freshness, observed_digest) =
                basis_applicability_fingerprint(connection, &agreement.basis_id)?;
            put_digest_field(&mut hasher, freshness.as_str().as_bytes());
            put_optional_digest_field(&mut hasher, observed_digest.as_deref());
            references.push(CompositionAgreementRef {
                ordinal: u32::try_from(references.len()).map_err(|_| {
                    StoreError::CorruptState("composition reference overflow".into())
                })?,
                track_id: track.track_id.clone(),
                agreement_id: agreement.agreement_id,
                terms_sha256: agreement.offer.terms_sha256,
                basis_id: agreement.basis_id,
            });
        } else {
            hasher.update([0]);
        }
    }
    Ok((roster, references, format!("{:x}", hasher.finalize())))
}

fn insert_composition_review(
    transaction: &Transaction<'_>,
    integration_id: &str,
    expected_composition_digest: &str,
    outcome: CompositionOutcome,
    review_markdown: &OpaqueMarkdown,
    attempt_id: Option<&str>,
    recorded_at: i64,
) -> StoreResult<String> {
    let (roster, agreements, current_digest) = composition_basis(transaction, integration_id)?;
    if current_digest != expected_composition_digest {
        return Err(StoreError::Stale(format!(
            "integration {integration_id} composition basis changed"
        )));
    }
    if outcome == CompositionOutcome::Compatible
        && agreements.len() != roster.tracks.iter().filter(|track| track.active).count()
    {
        return Err(StoreError::Conflict(
            "a compatible review requires an agreement for every active track".into(),
        ));
    }
    let review_id = new_id("cmp");
    transaction.execute(
        "INSERT INTO composition_reviews (
            review_id, integration_id, roster_revision, roster_digest,
            outcome, review_markdown, review_sha256, attempt_id, recorded_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            review_id,
            integration_id,
            roster.revision,
            current_digest,
            outcome.as_str(),
            review_markdown.as_bytes(),
            review_markdown.sha256(),
            attempt_id,
            recorded_at,
        ],
    )?;
    for reference in agreements {
        transaction.execute(
            "INSERT INTO composition_review_agreements (
                review_id, ordinal, track_id, agreement_id, terms_sha256, basis_id
             ) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                review_id,
                reference.ordinal,
                reference.track_id,
                reference.agreement_id,
                reference.terms_sha256,
                reference.basis_id,
            ],
        )?;
    }
    Ok(review_id)
}

fn read_composition_review(
    connection: &Connection,
    review_id: &str,
) -> StoreResult<CompositionReviewView> {
    let row = connection.query_row(
        "SELECT review_id, integration_id, roster_revision, roster_digest,
                outcome, review_markdown, review_sha256, attempt_id, recorded_at
         FROM composition_reviews WHERE review_id = ?",
        [review_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Vec<u8>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, i64>(8)?,
            ))
        },
    );
    let (id, integration_id, revision, digest, outcome, markdown, markdown_sha, attempt, at) =
        match row {
            Ok(value) => value,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                return Err(StoreError::NotFound(format!(
                    "composition review {review_id} does not exist"
                )));
            }
            Err(error) => return Err(error.into()),
        };
    let mut statement = connection.prepare(
        "SELECT ordinal, track_id, agreement_id, terms_sha256, basis_id
         FROM composition_review_agreements WHERE review_id = ? ORDER BY ordinal",
    )?;
    let agreements = collect_rows(statement.query_map([review_id], |row| {
        Ok(CompositionAgreementRef {
            ordinal: row.get(0)?,
            track_id: row.get(1)?,
            agreement_id: row.get(2)?,
            terms_sha256: row.get(3)?,
            basis_id: row.get(4)?,
        })
    })?)?;
    let current_digest = composition_basis(connection, &integration_id)?.2;
    Ok(CompositionReviewView {
        review_id: id,
        integration_id,
        roster_revision: revision,
        roster_digest: digest.clone(),
        outcome: CompositionOutcome::parse(&outcome)
            .ok_or_else(|| StoreError::CorruptState("invalid composition outcome".into()))?,
        review_markdown: decode_markdown(markdown)?,
        review_sha256: markdown_sha,
        attempt_id: attempt,
        agreements,
        stale: current_digest != digest,
        recorded_at: at,
    })
}

fn read_conformance_review(
    connection: &Connection,
    review_id: &str,
) -> StoreResult<ConformanceReviewView> {
    let row = connection.query_row(
        "SELECT review_id, agreement_id, candidate_basis_id, outcome,
                review_markdown, review_sha256, attempt_id, recorded_at
         FROM conformance_reviews WHERE review_id = ?",
        [review_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Vec<u8>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, i64>(7)?,
            ))
        },
    );
    match row {
        Ok((id, agreement, basis, outcome, markdown, markdown_sha, attempt, at)) => {
            Ok(ConformanceReviewView {
                review_id: id,
                agreement_id: agreement,
                candidate_basis_id: basis.clone(),
                outcome: ConformanceOutcome::parse(&outcome).ok_or_else(|| {
                    StoreError::CorruptState("invalid conformance outcome".into())
                })?,
                review_markdown: decode_markdown(markdown)?,
                review_sha256: markdown_sha,
                attempt_id: attempt,
                candidate_freshness: basis_freshness(connection, &basis)?,
                recorded_at: at,
            })
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Err(StoreError::NotFound(format!(
            "conformance review {review_id} does not exist"
        ))),
        Err(error) => Err(error.into()),
    }
}

#[allow(clippy::too_many_lines)]
fn validate_new_attempt(input: &NewAgentAttempt) -> StoreResult<()> {
    validate_text("attempt subject", &input.subject_id)?;
    validate_text("requester id", &input.requester_id)?;
    validate_text("Nucleus job id", &input.nucleus_job_id)?;
    validate_text("toolset name", &input.toolset_name)?;
    validate_text("catalog scope", &input.catalog_scope)?;
    validate_text("catalog verifier version", &input.catalog_verifier_version)?;
    validate_text("catalog party", &input.catalog_party)?;
    validate_text("catalog title", &input.catalog_title)?;
    validate_digest("request digest", &input.request_sha256)?;
    validate_digest("basis digest", &input.basis_digest)?;
    validate_digest("catalog charter digest", &input.catalog_charter_sha256)?;
    validate_digest("catalog digest", &input.catalog_sha256)?;
    if input.request_bytes.is_empty()
        || input.request_bytes.len() > MAX_AGENT_REQUEST_BYTES
        || sha256_hex(&input.request_bytes) != input.request_sha256
    {
        return Err(StoreError::InvalidInput(format!(
            "request must contain 1-{MAX_AGENT_REQUEST_BYTES} exact bytes matching its digest"
        )));
    }
    if input.catalog_charter_markdown.sha256() != input.catalog_charter_sha256 {
        return Err(StoreError::InvalidInput(
            "catalog charter digest does not match its exact Markdown bytes".into(),
        ));
    }
    if input.toolset_version == 0 || input.catalog_version == 0 || input.catalog_observed_at <= 0 {
        return Err(StoreError::InvalidInput(
            "toolset version, catalog version, and catalog observation time must be positive"
                .into(),
        ));
    }
    match input.kind {
        AttemptKind::StewardResponse => {
            if input.expected_offer_id.is_none()
                || input.basis_id.is_none()
                || input.expected_roster_digest.is_some()
            {
                return Err(StoreError::InvalidInput(
                    "steward attempt requires an offer and basis, not a roster".into(),
                ));
            }
        }
        AttemptKind::CompositionReview => {
            if input.expected_roster_digest.is_none()
                || input.expected_offer_id.is_some()
                || input.basis_id.is_some()
            {
                return Err(StoreError::InvalidInput(
                    "composition attempt requires only a composition digest".into(),
                ));
            }
        }
        AttemptKind::ConformanceReview => {
            if input.basis_id.is_none()
                || input.expected_offer_id.is_some()
                || input.expected_roster_digest.is_some()
            {
                return Err(StoreError::InvalidInput(
                    "conformance attempt requires only a candidate basis".into(),
                ));
            }
        }
    }
    if let Some(digest) = &input.expected_roster_digest {
        validate_digest("expected composition digest", digest)?;
    }
    if input.sources.is_empty() || input.sources.len() > MAX_FROZEN_SOURCES {
        return Err(StoreError::InvalidInput(format!(
            "attempt catalog requires 1-{MAX_FROZEN_SOURCES} sources"
        )));
    }
    let mut source_ids = BTreeSet::new();
    let mut total_bytes = 0_usize;
    for source in &input.sources {
        validate_text("attempt source id", &source.source_id)?;
        validate_text("attempt source kind", &source.kind)?;
        validate_text("attempt source locator", &source.locator)?;
        validate_text("attempt source origin path", &source.origin_path)?;
        if let Some(revision) = &source.revision {
            validate_text("attempt source revision", revision)?;
        }
        validate_digest("attempt source digest", &source.content_sha256)?;
        if source.observed_at <= 0
            || source.content.len() > MAX_FROZEN_SOURCE_BYTES
            || std::str::from_utf8(&source.content).is_err()
            || sha256_hex(&source.content) != source.content_sha256
        {
            return Err(StoreError::InvalidInput(format!(
                "attempt source {} bytes do not match their declared UTF-8 digest",
                source.source_id
            )));
        }
        total_bytes = total_bytes
            .checked_add(source.content.len())
            .ok_or_else(|| {
                StoreError::InvalidInput("attempt source byte total overflowed".into())
            })?;
        if total_bytes > MAX_FROZEN_CATALOG_BYTES {
            return Err(StoreError::InvalidInput(format!(
                "attempt sources exceed the {MAX_FROZEN_CATALOG_BYTES}-byte catalog limit"
            )));
        }
        if !source_ids.insert(source.source_id.as_str()) {
            return Err(StoreError::InvalidInput(format!(
                "attempt source {} is duplicated",
                source.source_id
            )));
        }
    }
    Ok(())
}

fn validate_attempt_target(connection: &Connection, input: &NewAgentAttempt) -> StoreResult<()> {
    match input.kind {
        AttemptKind::StewardResponse => {
            let track = require_open_negotiation(connection, &input.subject_id)?;
            let expected_offer = input
                .expected_offer_id
                .as_deref()
                .ok_or_else(|| StoreError::CorruptState("validated offer is missing".into()))?;
            require_expected_head(connection, &input.subject_id, expected_offer)?;
            let basis_id = input
                .basis_id
                .as_deref()
                .ok_or_else(|| StoreError::CorruptState("validated basis is missing".into()))?;
            require_steward_basis(
                connection,
                &track,
                &BasisGuard {
                    basis_id: basis_id.to_owned(),
                    observed_manifest_sha256: input.basis_digest.clone(),
                },
            )?;
            Ok(())
        }
        AttemptKind::CompositionReview => {
            let current = composition_basis(connection, &input.subject_id)?.2;
            let expected = input.expected_roster_digest.as_deref().ok_or_else(|| {
                StoreError::CorruptState("validated composition digest is missing".into())
            })?;
            if current != expected || input.basis_digest != expected {
                return Err(StoreError::Stale(
                    "integration composition basis changed before attempt persistence".into(),
                ));
            }
            Ok(())
        }
        AttemptKind::ConformanceReview => {
            read_agreement(connection, &input.subject_id)?;
            let basis_id = input
                .basis_id
                .as_deref()
                .ok_or_else(|| StoreError::CorruptState("validated basis is missing".into()))?;
            let basis = read_basis(connection, basis_id)?;
            if basis.kind != BasisKind::Candidate {
                return Err(StoreError::Conflict(
                    "conformance attempt basis is not a candidate".into(),
                ));
            }
            require_basis_digest(&basis, &input.basis_digest)
        }
    }
}

fn decode_attempt_kind(value: &str) -> StoreResult<AttemptKind> {
    AttemptKind::parse(value).ok_or_else(|| StoreError::CorruptState("invalid attempt kind".into()))
}

fn decode_runtime_state(value: &str) -> StoreResult<RuntimeState> {
    RuntimeState::parse(value)
        .ok_or_else(|| StoreError::CorruptState("invalid attempt runtime state".into()))
}

#[allow(clippy::too_many_lines)]
fn read_attempt(connection: &Connection, attempt_id: &str) -> StoreResult<AgentAttemptView> {
    struct RawAttempt {
        attempt_id: String,
        predecessor_attempt_id: Option<String>,
        kind: String,
        subject_id: String,
        requester_id: String,
        nucleus_job_id: String,
        request_bytes: Vec<u8>,
        request_sha256: String,
        toolset_name: String,
        toolset_version: u32,
        expected_offer_id: Option<String>,
        expected_roster_digest: Option<String>,
        basis_id: Option<String>,
        basis_digest: String,
        catalog_scope: String,
        catalog_version: u32,
        catalog_verifier_version: String,
        catalog_observed_at: i64,
        catalog_party: String,
        catalog_title: String,
        catalog_charter_markdown: Vec<u8>,
        catalog_charter_sha256: String,
        catalog_sha256: String,
        tool_after: i64,
        admitted: bool,
        accepted_job_id: Option<String>,
        accepted_request_sha256: Option<String>,
        active: bool,
        runtime_state: String,
        runtime_detail: Option<String>,
        domain_result_kind: Option<String>,
        domain_result_id: Option<String>,
        created_at: i64,
        updated_at: i64,
    }
    let row = connection.query_row(
        "SELECT attempt_id, predecessor_attempt_id, kind, subject_id, requester_id,
                nucleus_job_id, request_bytes, request_sha256, toolset_name, toolset_version,
                expected_offer_id, expected_roster_digest, basis_id, basis_digest,
                catalog_scope, catalog_version, catalog_verifier_version,
                catalog_observed_at, catalog_party, catalog_title,
                catalog_charter_markdown, catalog_charter_sha256, catalog_sha256,
                tool_after, admitted, accepted_job_id, accepted_request_sha256,
                active, runtime_state, runtime_detail, domain_result_kind,
                domain_result_id, created_at, updated_at
         FROM agent_attempts WHERE attempt_id = ?",
        [attempt_id],
        |row| {
            Ok(RawAttempt {
                attempt_id: row.get(0)?,
                predecessor_attempt_id: row.get(1)?,
                kind: row.get(2)?,
                subject_id: row.get(3)?,
                requester_id: row.get(4)?,
                nucleus_job_id: row.get(5)?,
                request_bytes: row.get(6)?,
                request_sha256: row.get(7)?,
                toolset_name: row.get(8)?,
                toolset_version: row.get(9)?,
                expected_offer_id: row.get(10)?,
                expected_roster_digest: row.get(11)?,
                basis_id: row.get(12)?,
                basis_digest: row.get(13)?,
                catalog_scope: row.get(14)?,
                catalog_version: row.get(15)?,
                catalog_verifier_version: row.get(16)?,
                catalog_observed_at: row.get(17)?,
                catalog_party: row.get(18)?,
                catalog_title: row.get(19)?,
                catalog_charter_markdown: row.get(20)?,
                catalog_charter_sha256: row.get(21)?,
                catalog_sha256: row.get(22)?,
                tool_after: row.get(23)?,
                admitted: row.get(24)?,
                accepted_job_id: row.get(25)?,
                accepted_request_sha256: row.get(26)?,
                active: row.get(27)?,
                runtime_state: row.get(28)?,
                runtime_detail: row.get(29)?,
                domain_result_kind: row.get(30)?,
                domain_result_id: row.get(31)?,
                created_at: row.get(32)?,
                updated_at: row.get(33)?,
            })
        },
    );
    let row = match row {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(StoreError::NotFound(format!(
                "attempt {attempt_id} does not exist"
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let mut statement = connection.prepare(
        "SELECT source_id, kind, locator, origin_path, revision,
                content, content_sha256, observed_at
         FROM attempt_sources WHERE attempt_id = ? ORDER BY ordinal",
    )?;
    let sources = collect_rows(statement.query_map([attempt_id], |row| {
        Ok(AttemptSourceInput {
            source_id: row.get(0)?,
            kind: row.get(1)?,
            locator: row.get(2)?,
            origin_path: row.get(3)?,
            revision: row.get(4)?,
            content: row.get(5)?,
            content_sha256: row.get(6)?,
            observed_at: row.get(7)?,
        })
    })?)?;
    Ok(AgentAttemptView {
        attempt_id: row.attempt_id,
        predecessor_attempt_id: row.predecessor_attempt_id,
        kind: decode_attempt_kind(&row.kind)?,
        subject_id: row.subject_id,
        requester_id: row.requester_id,
        nucleus_job_id: row.nucleus_job_id,
        request_bytes: row.request_bytes,
        request_sha256: row.request_sha256,
        toolset_name: row.toolset_name,
        toolset_version: row.toolset_version,
        expected_offer_id: row.expected_offer_id,
        expected_roster_digest: row.expected_roster_digest,
        basis_id: row.basis_id,
        basis_digest: row.basis_digest,
        catalog_scope: row.catalog_scope,
        catalog_version: row.catalog_version,
        catalog_verifier_version: row.catalog_verifier_version,
        catalog_observed_at: row.catalog_observed_at,
        catalog_party: row.catalog_party,
        catalog_title: row.catalog_title,
        catalog_charter_markdown: decode_markdown(row.catalog_charter_markdown)?,
        catalog_charter_sha256: row.catalog_charter_sha256,
        catalog_sha256: row.catalog_sha256,
        tool_after: u64::try_from(row.tool_after)
            .map_err(|_| StoreError::CorruptState("negative tool cursor".into()))?,
        admitted: row.admitted,
        accepted_job_id: row.accepted_job_id,
        accepted_request_sha256: row.accepted_request_sha256,
        active: row.active,
        runtime_state: decode_runtime_state(&row.runtime_state)?,
        runtime_detail: row.runtime_detail,
        domain_result_kind: row.domain_result_kind,
        domain_result_id: row.domain_result_id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        sources,
    })
}

fn attempt_matches_input(attempt: &AgentAttemptView, input: &NewAgentAttempt) -> bool {
    attempt.predecessor_attempt_id == input.predecessor_attempt_id
        && attempt.kind == input.kind
        && attempt.subject_id == input.subject_id
        && attempt.requester_id == input.requester_id
        && attempt.nucleus_job_id == input.nucleus_job_id
        && attempt.request_bytes == input.request_bytes
        && attempt.request_sha256 == input.request_sha256
        && attempt.toolset_name == input.toolset_name
        && attempt.toolset_version == input.toolset_version
        && attempt.expected_offer_id == input.expected_offer_id
        && attempt.expected_roster_digest == input.expected_roster_digest
        && attempt.basis_id == input.basis_id
        && attempt.basis_digest == input.basis_digest
        && attempt.catalog_scope == input.catalog_scope
        && attempt.catalog_version == input.catalog_version
        && attempt.catalog_verifier_version == input.catalog_verifier_version
        && attempt.catalog_observed_at == input.catalog_observed_at
        && attempt.catalog_party == input.catalog_party
        && attempt.catalog_title == input.catalog_title
        && attempt.catalog_charter_markdown == input.catalog_charter_markdown
        && attempt.catalog_charter_sha256 == input.catalog_charter_sha256
        && attempt.catalog_sha256 == input.catalog_sha256
        && attempt.sources == input.sources
}

fn read_tool_receipt_optional(
    connection: &Connection,
    nucleus_job_id: &str,
    call_id: &str,
) -> StoreResult<Option<ToolReceiptView>> {
    let receipt_id: Option<String> = connection
        .query_row(
            "SELECT receipt_id FROM tool_receipts
             WHERE nucleus_job_id = ? AND call_id = ?",
            params![nucleus_job_id, call_id],
            |row| row.get(0),
        )
        .optional()?;
    receipt_id
        .map(|receipt_id| read_tool_receipt_by_id(connection, &receipt_id))
        .transpose()
}

fn read_tool_receipt_by_id(
    connection: &Connection,
    receipt_id: &str,
) -> StoreResult<ToolReceiptView> {
    let row = connection.query_row(
        "SELECT receipt_id, attempt_id, nucleus_job_id, call_id,
                arguments_sha256, result_json, is_error,
                domain_result_kind, domain_result_id, recorded_at
         FROM tool_receipts WHERE receipt_id = ?",
        [receipt_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Vec<u8>>(5)?,
                row.get::<_, bool>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, i64>(9)?,
            ))
        },
    );
    let (id, attempt, job, call, arguments, result, error, kind, domain_id, at) = match row {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(StoreError::NotFound(format!(
                "tool receipt {receipt_id} does not exist"
            )));
        }
        Err(error) => return Err(error.into()),
    };
    let mut statement = connection.prepare(
        "SELECT source_ref FROM tool_receipt_source_refs
         WHERE receipt_id = ? ORDER BY source_ref",
    )?;
    let emitted_source_refs = collect_rows(statement.query_map([receipt_id], |row| row.get(0))?)?;
    Ok(ToolReceiptView {
        receipt_id: id,
        attempt_id: attempt,
        nucleus_job_id: job,
        call_id: call,
        arguments_sha256: arguments,
        result_json: result,
        is_error: error,
        domain_result_kind: kind,
        domain_result_id: domain_id,
        emitted_source_refs,
        recorded_at: at,
    })
}

#[allow(clippy::too_many_arguments)]
fn insert_tool_receipt(
    transaction: &Transaction<'_>,
    attempt: &AgentAttemptView,
    call_id: &str,
    arguments_sha256: &str,
    result_json: &[u8],
    is_error: bool,
    emitted_source_refs: &[String],
    domain_result: Option<(&str, &str)>,
) -> StoreResult<ToolReceiptView> {
    validate_text("tool call id", call_id)?;
    validate_digest("tool arguments digest", arguments_sha256)?;
    if result_json.is_empty()
        || result_json.len() > MAX_TOOL_RESULT_BYTES
        || serde_json::from_slice::<serde_json::Value>(result_json).is_err()
    {
        return Err(StoreError::InvalidInput(format!(
            "tool result must be 1-{MAX_TOOL_RESULT_BYTES} exact valid JSON bytes"
        )));
    }
    let receipt_id = new_id("rcp");
    let recorded_at = now_unix();
    transaction.execute(
        "INSERT INTO tool_receipts (
            receipt_id, attempt_id, nucleus_job_id, call_id,
            arguments_sha256, result_json, is_error,
            domain_result_kind, domain_result_id, recorded_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            receipt_id,
            attempt.attempt_id,
            attempt.nucleus_job_id,
            call_id,
            arguments_sha256,
            result_json,
            is_error,
            domain_result.map(|value| value.0),
            domain_result.map(|value| value.1),
            recorded_at,
        ],
    )?;
    let mut unique_refs = BTreeSet::new();
    for source_ref in emitted_source_refs {
        validate_text("emitted source reference", source_ref)?;
        if unique_refs.insert(source_ref) {
            transaction.execute(
                "INSERT INTO tool_receipt_source_refs (receipt_id, source_ref) VALUES (?, ?)",
                params![receipt_id, source_ref],
            )?;
        }
    }
    read_tool_receipt_by_id(transaction, &receipt_id)
}

fn verify_receipt_replay(
    existing: &ToolReceiptView,
    arguments_sha256: &str,
    result_json: &[u8],
    is_error: bool,
    domain_result: Option<(&str, &str)>,
) -> StoreResult<()> {
    if existing.arguments_sha256 != arguments_sha256
        || existing.result_json != result_json
        || existing.is_error != is_error
        || existing.domain_result_kind.as_deref() != domain_result.map(|value| value.0)
        || existing.domain_result_id.as_deref() != domain_result.map(|value| value.1)
    {
        return Err(StoreError::Conflict(
            "tool call was replayed with conflicting arguments or result".into(),
        ));
    }
    Ok(())
}

fn require_prior_source_refs(
    connection: &Connection,
    attempt_id: &str,
    cited_source_refs: &[String],
) -> StoreResult<()> {
    for source_ref in cited_source_refs {
        validate_text("cited source reference", source_ref)?;
        let emitted: bool = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM tool_receipt_source_refs r
                JOIN tool_receipts t ON t.receipt_id = r.receipt_id
                WHERE t.attempt_id = ? AND r.source_ref = ?
             )",
            params![attempt_id, source_ref],
            |row| row.get(0),
        )?;
        if !emitted {
            return Err(StoreError::Conflict(format!(
                "cited source reference was not emitted by a persisted source receipt: {source_ref}"
            )));
        }
    }
    Ok(())
}

fn require_attempt_kind(attempt: &AgentAttemptView, kind: AttemptKind) -> StoreResult<()> {
    if attempt.kind != kind {
        return Err(StoreError::Conflict(format!(
            "attempt {} has the wrong operation kind",
            attempt.attempt_id
        )));
    }
    Ok(())
}

#[cfg(test)]
fn require_attempt_kind_subject(
    connection: &Connection,
    attempt_id: &str,
    kind: AttemptKind,
    subject_id: &str,
) -> StoreResult<()> {
    let attempt = read_attempt(connection, attempt_id)?;
    require_attempt_kind(&attempt, kind)?;
    if attempt.subject_id != subject_id {
        return Err(StoreError::Conflict(format!(
            "attempt {attempt_id} belongs to a different subject"
        )));
    }
    Ok(())
}

fn bind_attempt_domain_result(
    transaction: &Transaction<'_>,
    attempt_id: &str,
    kind: &str,
    domain_id: &str,
    updated_at: i64,
) -> StoreResult<()> {
    let attempt = read_attempt(transaction, attempt_id)?;
    if !attempt.active {
        return Err(StoreError::Conflict(format!(
            "attempt {attempt_id} is no longer active"
        )));
    }
    match (
        attempt.domain_result_kind.as_deref(),
        attempt.domain_result_id.as_deref(),
    ) {
        (None, None) => {
            transaction.execute(
                "UPDATE agent_attempts
                 SET domain_result_kind = ?, domain_result_id = ?, updated_at = ?
                 WHERE attempt_id = ?",
                params![kind, domain_id, updated_at, attempt_id],
            )?;
        }
        (Some(existing_kind), Some(existing_id))
            if existing_kind == kind && existing_id == domain_id => {}
        _ => {
            return Err(StoreError::Conflict(format!(
                "attempt {attempt_id} already has a different domain result"
            )));
        }
    }
    Ok(())
}

fn terminal_result_json(kind: &str, id: &str, status: &str) -> StoreResult<Vec<u8>> {
    serde_json::to_vec(&serde_json::json!({
        "ok": true,
        "recorded": {
            "kind": kind,
            "id": id,
            "status": status,
        }
    }))
    .map_err(|error| StoreError::CorruptState(format!("cannot encode tool receipt: {error}")))
}

#[allow(clippy::too_many_arguments)]
fn apply_steward_response_transaction(
    transaction: &Transaction<'_>,
    negotiation_id: &str,
    expected_head_id: &str,
    basis_guard: &BasisGuard,
    response: &StewardResponse,
    attempt_id: Option<&str>,
    recorded_at: i64,
) -> StoreResult<(String, String, String)> {
    let track = require_open_negotiation(transaction, negotiation_id)?;
    require_expected_head(transaction, negotiation_id, expected_head_id)?;
    require_steward_basis(transaction, &track, basis_guard)?;
    insert_basis_verification(
        transaction,
        &basis_guard.basis_id,
        Some(&basis_guard.observed_manifest_sha256),
        None,
    )?;
    match response {
        StewardResponse::Assent { review_markdown } => {
            let event_id = append_negotiation_event(
                transaction,
                negotiation_id,
                NegotiationEventKind::Assent,
                Some(PartyRole::Steward),
                Some(expected_head_id),
                Some(&basis_guard.basis_id),
                Some(review_markdown),
                None,
                attempt_id,
                recorded_at,
            )?;
            if let Some(agreement_id) = maybe_seal(
                transaction,
                negotiation_id,
                &track,
                Some(basis_guard),
                recorded_at,
            )? {
                Ok(("agreement".into(), agreement_id, "sealed".into()))
            } else {
                Ok((
                    "steward_response".into(),
                    format!("evt_{event_id}"),
                    "assented".into(),
                ))
            }
        }
        StewardResponse::Counterproposal {
            terms_markdown,
            review_markdown,
        } => {
            let offer_id = new_id("off");
            insert_offer_event_with_review(
                transaction,
                &offer_id,
                negotiation_id,
                PartyRole::Steward,
                terms_markdown,
                Some(&basis_guard.basis_id),
                Some(review_markdown),
                attempt_id,
                recorded_at,
            )?;
            Ok(("offer".into(), offer_id, "counterproposal".into()))
        }
        StewardResponse::Blocked { review_markdown } => {
            let event_id = append_negotiation_event(
                transaction,
                negotiation_id,
                NegotiationEventKind::StewardBlocked,
                Some(PartyRole::Steward),
                Some(expected_head_id),
                Some(&basis_guard.basis_id),
                Some(review_markdown),
                None,
                attempt_id,
                recorded_at,
            )?;
            Ok((
                "steward_response".into(),
                format!("evt_{event_id}"),
                "blocked".into(),
            ))
        }
    }
}

impl Store {
    #[allow(dead_code)]
    fn add_track(
        &mut self,
        integration_id: &str,
        scope_id: &str,
        scope_version: u32,
    ) -> StoreResult<TrackView> {
        let track_id = new_id("trk");
        let now = now_unix();
        let transaction = self.immediate()?;
        require_integration(&transaction, integration_id)?;
        let scope = read_scope(&transaction, scope_id, scope_version)?;
        let duplicate: Option<String> = transaction
            .query_row(
                "SELECT t.track_id
                 FROM integration_tracks t
                 WHERE t.integration_id = ? AND t.scope_id = ?
                   AND NOT EXISTS (
                     SELECT 1 FROM integration_events e
                     WHERE e.integration_id = t.integration_id
                       AND e.track_id = t.track_id AND e.kind = 'track_retired'
                   )",
                params![integration_id, scope_id],
                |row| row.get(0),
            )
            .optional()?;
        if duplicate.is_some() {
            return Err(StoreError::Conflict(format!(
                "integration {integration_id} already has an active {scope_id} track"
            )));
        }
        transaction.execute(
            "INSERT INTO integration_tracks (
                track_id, integration_id, scope_id, scope_version,
                steward_party, created_at
             ) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                track_id,
                integration_id,
                scope_id,
                scope_version,
                scope.steward_party,
                now,
            ],
        )?;
        let ordinal = next_integration_ordinal(&transaction, integration_id)?;
        transaction.execute(
            "INSERT INTO integration_events (
                integration_id, ordinal, kind, track_id, recorded_at
             ) VALUES (?, ?, 'track_added', ?, ?)",
            params![integration_id, ordinal, track_id, now],
        )?;
        transaction.commit()?;
        self.track(&track_id)
    }

    pub fn retire_track(&mut self, track_id: &str, reason: &str) -> StoreResult<TrackView> {
        validate_text("retirement reason", reason)?;
        let now = now_unix();
        let transaction = self.immediate()?;
        let track = read_track(&transaction, track_id)?;
        if !track.active {
            return Err(StoreError::Conflict(format!(
                "track {track_id} is already retired"
            )));
        }
        if open_negotiation_id(&transaction, track_id)?.is_some() {
            return Err(StoreError::Conflict(format!(
                "track {track_id} has an open negotiation"
            )));
        }
        let ordinal = next_integration_ordinal(&transaction, &track.integration_id)?;
        transaction.execute(
            "INSERT INTO integration_events (
                integration_id, ordinal, kind, track_id, reason, recorded_at
             ) VALUES (?, ?, 'track_retired', ?, ?, ?)",
            params![track.integration_id, ordinal, track_id, reason, now],
        )?;
        transaction.commit()?;
        self.track(track_id)
    }

    pub fn open_track(
        &mut self,
        integration_id: &str,
        scope_id: &str,
        scope_version: u32,
        initial_terms: &OpaqueMarkdown,
    ) -> StoreResult<(TrackView, NegotiationView)> {
        let now = now_unix();
        let track_id = new_id("trk");
        let negotiation_id = new_id("neg");
        let offer_id = new_id("off");
        let transaction = self.immediate()?;
        require_integration(&transaction, integration_id)?;
        let scope = read_scope(&transaction, scope_id, scope_version)?;
        let existing_track_id: Option<String> = transaction
            .query_row(
                "SELECT t.track_id
                 FROM integration_tracks t
                 WHERE t.integration_id = ? AND t.scope_id = ?
                   AND NOT EXISTS (
                     SELECT 1 FROM integration_events e
                     WHERE e.integration_id = t.integration_id
                       AND e.track_id = t.track_id AND e.kind = 'track_retired'
                   )",
                params![integration_id, scope_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing_track_id) = existing_track_id {
            let existing = read_track(&transaction, &existing_track_id)?;
            if existing.scope_version != scope_version {
                return Err(StoreError::Conflict(format!(
                    "integration {integration_id} already has a different active {scope_id} version"
                )));
            }
            let existing_negotiation_id =
                latest_negotiation_for_track(&transaction, &existing_track_id)?.ok_or_else(
                    || {
                        StoreError::CorruptState(format!(
                            "active track {existing_track_id} has no negotiation"
                        ))
                    },
                )?;
            let first_terms_sha256: String = transaction.query_row(
                "SELECT o.terms_sha256
                 FROM offers o JOIN negotiation_events e ON e.offer_id = o.offer_id
                 WHERE o.negotiation_id = ? AND e.kind = 'offer_submitted'
                 ORDER BY e.ordinal LIMIT 1",
                [&existing_negotiation_id],
                |row| row.get(0),
            )?;
            if first_terms_sha256 != initial_terms.sha256() {
                return Err(StoreError::Conflict(format!(
                    "integration {integration_id} already has an active {scope_id} track"
                )));
            }
            transaction.commit()?;
            return Ok((
                self.track(&existing_track_id)?,
                self.negotiation(&existing_negotiation_id)?,
            ));
        }
        transaction.execute(
            "INSERT INTO integration_tracks (
                track_id, integration_id, scope_id, scope_version,
                steward_party, created_at
             ) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                track_id,
                integration_id,
                scope_id,
                scope_version,
                scope.steward_party,
                now,
            ],
        )?;
        let roster_ordinal = next_integration_ordinal(&transaction, integration_id)?;
        transaction.execute(
            "INSERT INTO integration_events (
                integration_id, ordinal, kind, track_id, recorded_at
             ) VALUES (?, ?, 'track_added', ?, ?)",
            params![integration_id, roster_ordinal, track_id, now],
        )?;
        insert_negotiation(
            &transaction,
            &negotiation_id,
            &track_id,
            NegotiationKind::Initial,
            None,
            now,
        )?;
        insert_offer_event(
            &transaction,
            &offer_id,
            &negotiation_id,
            PartyRole::Entrant,
            initial_terms,
            None,
            now,
        )?;
        transaction.commit()?;
        Ok((self.track(&track_id)?, self.negotiation(&negotiation_id)?))
    }

    pub fn track(&self, track_id: &str) -> StoreResult<TrackView> {
        read_track(&self.connection, track_id)
    }

    pub fn roster(&self, integration_id: &str) -> StoreResult<RosterView> {
        require_integration(&self.connection, integration_id)?;
        let revision: u32 = self.connection.query_row(
            "SELECT COALESCE(MAX(ordinal), 0) FROM integration_events WHERE integration_id = ?",
            [integration_id],
            |row| row.get(0),
        )?;
        let mut statement = self.connection.prepare(
            "SELECT t.track_id, t.integration_id, t.scope_id, t.scope_version,
                    t.steward_party, t.created_at,
                    NOT EXISTS (
                      SELECT 1 FROM integration_events e
                      WHERE e.integration_id = t.integration_id
                        AND e.track_id = t.track_id AND e.kind = 'track_retired'
                    ) AS active
             FROM integration_tracks t
             WHERE t.integration_id = ?
             ORDER BY t.track_id",
        )?;
        let rows = statement.query_map([integration_id], decode_track_row)?;
        let tracks = collect_rows(rows)?;
        let digest = roster_digest(integration_id, revision, &tracks);
        Ok(RosterView {
            integration_id: integration_id.into(),
            revision,
            digest,
            tracks,
        })
    }

    pub fn open_amendment(
        &mut self,
        agreement_id: &str,
        initial_terms: &OpaqueMarkdown,
    ) -> StoreResult<NegotiationView> {
        let predecessor = read_agreement(&self.connection, agreement_id)?;
        let negotiation_id = new_id("neg");
        let offer_id = new_id("off");
        let now = now_unix();
        let transaction = self.immediate()?;
        let track = read_track(&transaction, &predecessor.track_id)?;
        require_track_available(&transaction, &track)?;
        let current = active_agreement_for_track(&transaction, &track.track_id)?
            .ok_or_else(|| StoreError::CorruptState("agreement track has no agreement".into()))?;
        if current.agreement_id != agreement_id {
            return Err(StoreError::Conflict(format!(
                "agreement {agreement_id} is not the active agreement for its track"
            )));
        }
        insert_negotiation(
            &transaction,
            &negotiation_id,
            &track.track_id,
            NegotiationKind::Amendment,
            Some(agreement_id),
            now,
        )?;
        insert_offer_event(
            &transaction,
            &offer_id,
            &negotiation_id,
            PartyRole::Entrant,
            initial_terms,
            None,
            now,
        )?;
        transaction.commit()?;
        self.negotiation(&negotiation_id)
    }

    pub fn propose_as_entrant(
        &mut self,
        negotiation_id: &str,
        expected_head_id: &str,
        terms: &OpaqueMarkdown,
    ) -> StoreResult<MutationResult> {
        let offer_id = new_id("off");
        let now = now_unix();
        let transaction = self.immediate()?;
        require_open_negotiation(&transaction, negotiation_id)?;
        require_expected_head(&transaction, negotiation_id, expected_head_id)?;
        insert_offer_event(
            &transaction,
            &offer_id,
            negotiation_id,
            PartyRole::Entrant,
            terms,
            None,
            now,
        )?;
        transaction.commit()?;
        Ok(MutationResult {
            negotiation: self.negotiation(negotiation_id)?,
            offer_id: Some(offer_id),
            agreement_id: None,
        })
    }

    pub fn assent_as_entrant(
        &mut self,
        negotiation_id: &str,
        offer_id: &str,
        basis_guard: Option<&BasisGuard>,
    ) -> StoreResult<MutationResult> {
        let now = now_unix();
        let transaction = self.immediate()?;
        let track = require_open_negotiation(&transaction, negotiation_id)?;
        require_expected_head(&transaction, negotiation_id, offer_id)?;
        let current = effective_assent(&transaction, negotiation_id, PartyRole::Entrant)?;
        if current.as_ref().is_some_and(|record| {
            record.kind != NegotiationEventKind::AssentWithdrawn
                && record.offer_id.as_deref() == Some(offer_id)
        }) {
            return Err(StoreError::Conflict(
                "entrant already assents to the current terms".into(),
            ));
        }
        append_negotiation_event(
            &transaction,
            negotiation_id,
            NegotiationEventKind::Assent,
            Some(PartyRole::Entrant),
            Some(offer_id),
            None,
            None,
            None,
            None,
            now,
        )?;
        let agreement_id = maybe_seal(&transaction, negotiation_id, &track, basis_guard, now)?;
        transaction.commit()?;
        Ok(MutationResult {
            negotiation: self.negotiation(negotiation_id)?,
            offer_id: None,
            agreement_id,
        })
    }

    pub fn withdraw_entrant_assent(
        &mut self,
        negotiation_id: &str,
        offer_id: &str,
    ) -> StoreResult<NegotiationView> {
        let now = now_unix();
        let transaction = self.immediate()?;
        require_open_negotiation(&transaction, negotiation_id)?;
        require_expected_head(&transaction, negotiation_id, offer_id)?;
        let current = effective_assent(&transaction, negotiation_id, PartyRole::Entrant)?;
        if !current.as_ref().is_some_and(|record| {
            record.kind != NegotiationEventKind::AssentWithdrawn
                && record.offer_id.as_deref() == Some(offer_id)
        }) {
            return Err(StoreError::Conflict(
                "entrant does not currently assent to these terms".into(),
            ));
        }
        append_negotiation_event(
            &transaction,
            negotiation_id,
            NegotiationEventKind::AssentWithdrawn,
            Some(PartyRole::Entrant),
            Some(offer_id),
            None,
            None,
            None,
            None,
            now,
        )?;
        transaction.commit()?;
        self.negotiation(negotiation_id)
    }

    pub fn cancel_negotiation(
        &mut self,
        negotiation_id: &str,
        reason: &str,
    ) -> StoreResult<NegotiationView> {
        validate_text("cancellation reason", reason)?;
        let now = now_unix();
        let transaction = self.immediate()?;
        require_open_negotiation(&transaction, negotiation_id)?;
        append_negotiation_event(
            &transaction,
            negotiation_id,
            NegotiationEventKind::Cancelled,
            Some(PartyRole::Entrant),
            None,
            None,
            None,
            Some(reason),
            None,
            now,
        )?;
        transaction.commit()?;
        self.negotiation(negotiation_id)
    }

    #[cfg(test)]
    pub fn apply_steward_response(
        &mut self,
        negotiation_id: &str,
        expected_head_id: &str,
        basis_guard: &BasisGuard,
        response: &StewardResponse,
        attempt_id: Option<&str>,
    ) -> StoreResult<MutationResult> {
        let now = now_unix();
        let transaction = self.immediate()?;
        let track = require_open_negotiation(&transaction, negotiation_id)?;
        require_expected_head(&transaction, negotiation_id, expected_head_id)?;
        require_steward_basis(&transaction, &track, basis_guard)?;
        insert_basis_verification(
            &transaction,
            &basis_guard.basis_id,
            Some(&basis_guard.observed_manifest_sha256),
            None,
        )?;
        let mut offer_id = None;
        let agreement_id = match response {
            StewardResponse::Assent { review_markdown } => {
                append_negotiation_event(
                    &transaction,
                    negotiation_id,
                    NegotiationEventKind::Assent,
                    Some(PartyRole::Steward),
                    Some(expected_head_id),
                    Some(&basis_guard.basis_id),
                    Some(review_markdown),
                    None,
                    attempt_id,
                    now,
                )?;
                maybe_seal(&transaction, negotiation_id, &track, Some(basis_guard), now)?
            }
            StewardResponse::Counterproposal {
                terms_markdown,
                review_markdown,
            } => {
                let created_offer_id = new_id("off");
                insert_offer_event_with_review(
                    &transaction,
                    &created_offer_id,
                    negotiation_id,
                    PartyRole::Steward,
                    terms_markdown,
                    Some(&basis_guard.basis_id),
                    Some(review_markdown),
                    attempt_id,
                    now,
                )?;
                offer_id = Some(created_offer_id);
                None
            }
            StewardResponse::Blocked { review_markdown } => {
                append_negotiation_event(
                    &transaction,
                    negotiation_id,
                    NegotiationEventKind::StewardBlocked,
                    Some(PartyRole::Steward),
                    Some(expected_head_id),
                    Some(&basis_guard.basis_id),
                    Some(review_markdown),
                    None,
                    attempt_id,
                    now,
                )?;
                None
            }
        };
        transaction.commit()?;
        Ok(MutationResult {
            negotiation: self.negotiation(negotiation_id)?,
            offer_id,
            agreement_id,
        })
    }
}

impl Store {
    pub fn negotiation(&self, negotiation_id: &str) -> StoreResult<NegotiationView> {
        read_negotiation(&self.connection, negotiation_id)
    }

    pub fn negotiation_history(
        &self,
        negotiation_id: &str,
    ) -> StoreResult<Vec<NegotiationEventView>> {
        require_negotiation(&self.connection, negotiation_id)?;
        let mut statement = self.connection.prepare(
            "SELECT event_id, negotiation_id, ordinal, kind, party_role,
                    offer_id, basis_id, review_markdown, reason, attempt_id, recorded_at
             FROM negotiation_events
             WHERE negotiation_id = ? ORDER BY ordinal",
        )?;
        let rows = statement.query_map([negotiation_id], decode_negotiation_event_row)?;
        collect_rows(rows)
    }

    pub fn agreement(&self, agreement_id: &str) -> StoreResult<AgreementView> {
        read_agreement(&self.connection, agreement_id)
    }

    pub fn checked_agreement(&self, agreement_id: &str) -> StoreResult<AgreementView> {
        verify_agreement_integrity(&self.connection, agreement_id)
    }

    pub fn verify_agreement(
        &mut self,
        agreement_id: &str,
        observed_manifest_sha256: Option<&str>,
        detail_markdown: Option<&OpaqueMarkdown>,
    ) -> StoreResult<AgreementView> {
        if let Some(digest) = observed_manifest_sha256 {
            validate_digest("observed basis digest", digest)?;
        }
        let transaction = self.immediate()?;
        let agreement = verify_agreement_integrity(&transaction, agreement_id)?;
        insert_basis_verification(
            &transaction,
            &agreement.basis_id,
            observed_manifest_sha256,
            detail_markdown,
        )?;
        let verified = read_agreement(&transaction, agreement_id)?;
        transaction.commit()?;
        Ok(verified)
    }

    pub fn composition_basis(
        &self,
        integration_id: &str,
    ) -> StoreResult<(RosterView, Vec<CompositionAgreementRef>, String)> {
        composition_basis(&self.connection, integration_id)
    }

    #[cfg(test)]
    pub fn record_composition_review(
        &mut self,
        integration_id: &str,
        expected_composition_digest: &str,
        outcome: CompositionOutcome,
        review_markdown: &OpaqueMarkdown,
        attempt_id: Option<&str>,
    ) -> StoreResult<CompositionReviewView> {
        validate_digest("expected composition digest", expected_composition_digest)?;
        let review_id = new_id("cmp");
        let now = now_unix();
        let transaction = self.immediate()?;
        let (roster, agreements, current_digest) = composition_basis(&transaction, integration_id)?;
        if current_digest != expected_composition_digest {
            return Err(StoreError::Stale(format!(
                "integration {integration_id} composition basis changed"
            )));
        }
        if outcome == CompositionOutcome::Compatible
            && agreements.len() != roster.tracks.iter().filter(|track| track.active).count()
        {
            return Err(StoreError::Conflict(
                "a compatible review requires an agreement for every active track".into(),
            ));
        }
        if let Some(attempt_id) = attempt_id {
            require_attempt_kind_subject(
                &transaction,
                attempt_id,
                AttemptKind::CompositionReview,
                integration_id,
            )?;
        }
        transaction.execute(
            "INSERT INTO composition_reviews (
                review_id, integration_id, roster_revision, roster_digest,
                outcome, review_markdown, review_sha256, attempt_id, recorded_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                review_id,
                integration_id,
                roster.revision,
                current_digest,
                outcome.as_str(),
                review_markdown.as_bytes(),
                review_markdown.sha256(),
                attempt_id,
                now,
            ],
        )?;
        for reference in &agreements {
            transaction.execute(
                "INSERT INTO composition_review_agreements (
                    review_id, ordinal, track_id, agreement_id, terms_sha256, basis_id
                 ) VALUES (?, ?, ?, ?, ?, ?)",
                params![
                    review_id,
                    reference.ordinal,
                    reference.track_id,
                    reference.agreement_id,
                    reference.terms_sha256,
                    reference.basis_id,
                ],
            )?;
        }
        transaction.commit()?;
        self.composition_review(&review_id)
    }

    pub fn composition_review(&self, review_id: &str) -> StoreResult<CompositionReviewView> {
        read_composition_review(&self.connection, review_id)
    }

    pub fn latest_composition_review(
        &self,
        integration_id: &str,
    ) -> StoreResult<Option<CompositionReviewView>> {
        let review_id: Option<String> = self
            .connection
            .query_row(
                "SELECT review_id FROM composition_reviews
                 WHERE integration_id = ? ORDER BY recorded_at DESC, review_id DESC LIMIT 1",
                [integration_id],
                |row| row.get(0),
            )
            .optional()?;
        review_id.map(|id| self.composition_review(&id)).transpose()
    }

    pub fn conformance_review(&self, review_id: &str) -> StoreResult<ConformanceReviewView> {
        read_conformance_review(&self.connection, review_id)
    }

    pub fn integration_status(&self, integration_id: &str) -> StoreResult<IntegrationStatusView> {
        let integration = self.integration(integration_id)?;
        let roster = self.roster(integration_id)?;
        let mut tracks = Vec::new();
        for track in roster.tracks.iter().filter(|track| track.active) {
            let negotiation = latest_negotiation_for_track(&self.connection, &track.track_id)?
                .map(|id| self.negotiation(&id))
                .transpose()?;
            let active_agreement = active_agreement_for_track(&self.connection, &track.track_id)?;
            let renegotiating = negotiation
                .as_ref()
                .is_some_and(|value| value.status == NegotiationStatus::Open)
                && active_agreement.is_some();
            tracks.push(TrackStatusView {
                track: track.clone(),
                negotiation,
                active_agreement,
                renegotiating,
            });
        }
        let latest_composition_review = self.latest_composition_review(integration_id)?;
        let all_tracks_ready = !tracks.is_empty()
            && tracks.iter().all(|track| {
                track
                    .active_agreement
                    .as_ref()
                    .is_some_and(|agreement| agreement.basis_freshness == BasisFreshness::Fresh)
            });
        let composition_ready = latest_composition_review.as_ref().is_some_and(|review| {
            review.outcome == CompositionOutcome::Compatible && !review.stale
        });
        Ok(IntegrationStatusView {
            integration,
            roster,
            tracks,
            latest_composition_review,
            ready: all_tracks_ready && composition_ready,
        })
    }
}

impl Store {
    pub fn begin_or_resume_attempt(
        &mut self,
        input: &NewAgentAttempt,
    ) -> StoreResult<AgentAttemptView> {
        validate_new_attempt(input)?;
        let now = now_unix();
        let attempt_id = new_id("att");
        let transaction = self.immediate()?;
        let existing_id: Option<String> = transaction
            .query_row(
                "SELECT attempt_id FROM agent_attempts
                 WHERE kind = ? AND subject_id = ? AND active = 1",
                params![input.kind.as_str(), input.subject_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing_id) = existing_id {
            let attempt = read_attempt(&transaction, &existing_id)?;
            if !attempt_matches_input(&attempt, input) {
                return Err(StoreError::Conflict(format!(
                    "active attempt {existing_id} has a different immutable request"
                )));
            }
            transaction.commit()?;
            return Ok(attempt);
        }
        validate_attempt_target(&transaction, input)?;
        if let Some(predecessor_attempt_id) = &input.predecessor_attempt_id {
            let predecessor = read_attempt(&transaction, predecessor_attempt_id)?;
            if predecessor.kind != input.kind
                || predecessor.subject_id != input.subject_id
                || predecessor.active
                || !predecessor.runtime_state.is_terminal()
                || predecessor.domain_result_id.is_some()
            {
                return Err(StoreError::Conflict(
                    "attempt predecessor must be a terminal unsuccessful attempt for the same operation"
                        .into(),
                ));
            }
        }
        transaction.execute(
            "INSERT INTO agent_attempts (
                attempt_id, predecessor_attempt_id, kind, subject_id, requester_id, nucleus_job_id,
                request_bytes, request_sha256, toolset_name, toolset_version,
                expected_offer_id, expected_roster_digest, basis_id, basis_digest,
                catalog_scope, catalog_version, catalog_verifier_version,
                catalog_observed_at, catalog_party, catalog_title,
                catalog_charter_markdown, catalog_charter_sha256, catalog_sha256,
                created_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                attempt_id,
                input.predecessor_attempt_id,
                input.kind.as_str(),
                input.subject_id,
                input.requester_id,
                input.nucleus_job_id,
                input.request_bytes,
                input.request_sha256,
                input.toolset_name,
                input.toolset_version,
                input.expected_offer_id,
                input.expected_roster_digest,
                input.basis_id,
                input.basis_digest,
                input.catalog_scope,
                input.catalog_version,
                input.catalog_verifier_version,
                input.catalog_observed_at,
                input.catalog_party,
                input.catalog_title,
                input.catalog_charter_markdown.as_bytes(),
                input.catalog_charter_sha256,
                input.catalog_sha256,
                now,
                now,
            ],
        )?;
        for (ordinal, source) in input.sources.iter().enumerate() {
            transaction.execute(
                "INSERT INTO attempt_sources (
                    attempt_id, ordinal, source_id, kind, locator, origin_path,
                    revision, content, content_sha256, observed_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    attempt_id,
                    i64::try_from(ordinal)
                        .map_err(|_| StoreError::CorruptState("source ordinal overflow".into()))?,
                    source.source_id,
                    source.kind,
                    source.locator,
                    source.origin_path,
                    source.revision,
                    source.content,
                    source.content_sha256,
                    source.observed_at,
                ],
            )?;
        }
        let attempt = read_attempt(&transaction, &attempt_id)?;
        transaction.commit()?;
        Ok(attempt)
    }

    pub fn active_attempt(
        &self,
        kind: AttemptKind,
        subject_id: &str,
    ) -> StoreResult<Option<AgentAttemptView>> {
        let id: Option<String> = self
            .connection
            .query_row(
                "SELECT attempt_id FROM agent_attempts
                 WHERE kind = ? AND subject_id = ? AND active = 1",
                params![kind.as_str(), subject_id],
                |row| row.get(0),
            )
            .optional()?;
        id.map(|id| self.attempt(&id)).transpose()
    }

    pub fn latest_attempt(
        &self,
        kind: AttemptKind,
        subject_id: &str,
    ) -> StoreResult<Option<AgentAttemptView>> {
        let id: Option<String> = self
            .connection
            .query_row(
                "SELECT attempt_id FROM agent_attempts
                 WHERE kind = ? AND subject_id = ?
                 ORDER BY created_at DESC, attempt_id DESC LIMIT 1",
                params![kind.as_str(), subject_id],
                |row| row.get(0),
            )
            .optional()?;
        id.map(|id| self.attempt(&id)).transpose()
    }

    pub fn attempt(&self, attempt_id: &str) -> StoreResult<AgentAttemptView> {
        read_attempt(&self.connection, attempt_id)
    }

    pub fn attempt_emitted_source_refs(&self, attempt_id: &str) -> StoreResult<BTreeSet<String>> {
        self.attempt(attempt_id)?;
        let mut statement = self.connection.prepare(
            "SELECT r.source_ref
             FROM tool_receipt_source_refs r
             JOIN tool_receipts t ON t.receipt_id = r.receipt_id
             WHERE t.attempt_id = ? ORDER BY r.source_ref",
        )?;
        let refs = collect_rows(statement.query_map([attempt_id], |row| row.get(0))?)?;
        Ok(refs.into_iter().collect())
    }

    pub fn mark_attempt_admitted(
        &mut self,
        attempt_id: &str,
        accepted_job_id: &str,
        accepted_request_sha256: &str,
    ) -> StoreResult<AgentAttemptView> {
        validate_digest("accepted request digest", accepted_request_sha256)?;
        let now = now_unix();
        let transaction = self.immediate()?;
        let attempt = read_attempt(&transaction, attempt_id)?;
        if accepted_job_id != attempt.nucleus_job_id
            || accepted_request_sha256 != attempt.request_sha256
        {
            return Err(StoreError::Conflict(
                "Nucleus admission identity does not match the persisted request".into(),
            ));
        }
        if attempt.admitted {
            if attempt.accepted_job_id.as_deref() != Some(accepted_job_id)
                || attempt.accepted_request_sha256.as_deref() != Some(accepted_request_sha256)
            {
                return Err(StoreError::Conflict(
                    "attempt was already admitted with a different identity".into(),
                ));
            }
        } else {
            if !attempt.active {
                return Err(StoreError::Conflict(format!(
                    "attempt {attempt_id} is no longer active"
                )));
            }
            transaction.execute(
                "UPDATE agent_attempts
                 SET admitted = 1, accepted_job_id = ?, accepted_request_sha256 = ?,
                     runtime_state = 'admitted', updated_at = ?
                 WHERE attempt_id = ?",
                params![accepted_job_id, accepted_request_sha256, now, attempt_id],
            )?;
        }
        let view = read_attempt(&transaction, attempt_id)?;
        transaction.commit()?;
        Ok(view)
    }

    pub fn advance_attempt_tool_after(
        &mut self,
        attempt_id: &str,
        sequence: u64,
    ) -> StoreResult<AgentAttemptView> {
        let sequence = i64::try_from(sequence).map_err(|_| {
            StoreError::InvalidInput("tool mailbox sequence exceeds SQLite range".into())
        })?;
        let now = now_unix();
        self.connection.execute(
            "UPDATE agent_attempts
             SET tool_after = MAX(tool_after, ?), runtime_state = CASE
                    WHEN runtime_state IN ('prepared', 'admitted') THEN 'running'
                    ELSE runtime_state END,
                 updated_at = ?
             WHERE attempt_id = ?",
            params![sequence, now, attempt_id],
        )?;
        self.attempt(attempt_id)
    }

    pub fn mark_attempt_runtime_state(
        &mut self,
        attempt_id: &str,
        state: RuntimeState,
        detail: Option<&str>,
    ) -> StoreResult<AgentAttemptView> {
        let now = now_unix();
        let changed = self.connection.execute(
            "UPDATE agent_attempts
             SET runtime_state = ?, runtime_detail = ?,
                 active = CASE WHEN ? THEN 0 ELSE active END,
                 updated_at = ?
             WHERE attempt_id = ?",
            params![state.as_str(), detail, state.is_terminal(), now, attempt_id,],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(format!(
                "attempt {attempt_id} does not exist"
            )));
        }
        self.attempt(attempt_id)
    }

    pub fn tool_receipt(
        &self,
        nucleus_job_id: &str,
        call_id: &str,
    ) -> StoreResult<Option<ToolReceiptView>> {
        read_tool_receipt_optional(&self.connection, nucleus_job_id, call_id)
    }

    pub fn attempt_receipts(&self, attempt_id: &str) -> StoreResult<Vec<ToolReceiptView>> {
        self.attempt(attempt_id)?;
        let mut statement = self.connection.prepare(
            "SELECT receipt_id FROM tool_receipts
             WHERE attempt_id = ? ORDER BY recorded_at, call_id, receipt_id",
        )?;
        let ids = collect_rows(statement.query_map([attempt_id], |row| row.get::<_, String>(0))?)?;
        ids.into_iter()
            .map(|receipt_id| read_tool_receipt_by_id(&self.connection, &receipt_id))
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_tool_receipt(
        &mut self,
        attempt_id: &str,
        call_id: &str,
        arguments_sha256: &str,
        result_json: &[u8],
        is_error: bool,
        emitted_source_refs: &[String],
        domain_result: Option<(&str, &str)>,
    ) -> StoreResult<ToolReceiptView> {
        validate_digest("tool arguments digest", arguments_sha256)?;
        let transaction = self.immediate()?;
        let attempt = read_attempt(&transaction, attempt_id)?;
        if let Some(existing) =
            read_tool_receipt_optional(&transaction, &attempt.nucleus_job_id, call_id)?
        {
            verify_receipt_replay(
                &existing,
                arguments_sha256,
                result_json,
                is_error,
                domain_result,
            )?;
            transaction.commit()?;
            return Ok(existing);
        }
        if !attempt.active {
            return Err(StoreError::Conflict(format!(
                "attempt {attempt_id} is no longer active"
            )));
        }
        let receipt = insert_tool_receipt(
            &transaction,
            &attempt,
            call_id,
            arguments_sha256,
            result_json,
            is_error,
            emitted_source_refs,
            domain_result,
        )?;
        transaction.commit()?;
        Ok(receipt)
    }

    pub fn record_closed_tool_receipt(
        &mut self,
        attempt_id: &str,
        call_id: &str,
        arguments_sha256: &str,
        result_json: &[u8],
    ) -> StoreResult<ToolReceiptView> {
        validate_digest("tool arguments digest", arguments_sha256)?;
        let transaction = self.immediate()?;
        let attempt = read_attempt(&transaction, attempt_id)?;
        if let Some(existing) =
            read_tool_receipt_optional(&transaction, &attempt.nucleus_job_id, call_id)?
        {
            verify_receipt_replay(&existing, arguments_sha256, result_json, true, None)?;
            transaction.commit()?;
            return Ok(existing);
        }
        if attempt.active || attempt.domain_result_id.is_none() {
            return Err(StoreError::Conflict(format!(
                "attempt {attempt_id} is not closed with a committed result"
            )));
        }
        let receipt = insert_tool_receipt(
            &transaction,
            &attempt,
            call_id,
            arguments_sha256,
            result_json,
            true,
            &[],
            None,
        )?;
        transaction.commit()?;
        Ok(receipt)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_steward_tool_response(
        &mut self,
        attempt_id: &str,
        call_id: &str,
        arguments_sha256: &str,
        observed_current_basis_sha256: &str,
        response: &StewardResponse,
        cited_source_refs: &[String],
    ) -> StoreResult<ToolReceiptView> {
        validate_digest("tool arguments digest", arguments_sha256)?;
        validate_digest("observed basis digest", observed_current_basis_sha256)?;
        let now = now_unix();
        let transaction = self.immediate()?;
        let attempt = read_attempt(&transaction, attempt_id)?;
        require_attempt_kind(&attempt, AttemptKind::StewardResponse)?;
        if let Some(existing) =
            read_tool_receipt_optional(&transaction, &attempt.nucleus_job_id, call_id)?
        {
            if existing.arguments_sha256 != arguments_sha256 {
                return Err(StoreError::Conflict(
                    "tool call was replayed with different arguments".into(),
                ));
            }
            transaction.commit()?;
            return Ok(existing);
        }
        if !attempt.active {
            return Err(StoreError::Conflict(format!(
                "attempt {attempt_id} is no longer active"
            )));
        }
        require_prior_source_refs(&transaction, attempt_id, cited_source_refs)?;
        let expected_head = attempt.expected_offer_id.as_deref().ok_or_else(|| {
            StoreError::CorruptState("steward attempt has no expected offer".into())
        })?;
        let basis_id = attempt.basis_id.as_deref().ok_or_else(|| {
            StoreError::CorruptState("steward attempt has no frozen basis".into())
        })?;
        if attempt.basis_digest != observed_current_basis_sha256 {
            return Err(StoreError::Stale(
                "steward target basis changed during the attempt".into(),
            ));
        }
        let guard = BasisGuard {
            basis_id: basis_id.into(),
            observed_manifest_sha256: observed_current_basis_sha256.into(),
        };
        let (domain_kind, domain_id, domain_status) = apply_steward_response_transaction(
            &transaction,
            &attempt.subject_id,
            expected_head,
            &guard,
            response,
            Some(attempt_id),
            now,
        )?;
        let result_json = terminal_result_json(&domain_kind, &domain_id, &domain_status)?;
        bind_attempt_domain_result(&transaction, attempt_id, &domain_kind, &domain_id, now)?;
        let receipt = insert_tool_receipt(
            &transaction,
            &attempt,
            call_id,
            arguments_sha256,
            &result_json,
            false,
            cited_source_refs,
            Some((&domain_kind, &domain_id)),
        )?;
        transaction.commit()?;
        Ok(receipt)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_composition_tool_response(
        &mut self,
        attempt_id: &str,
        call_id: &str,
        arguments_sha256: &str,
        observed_basis_digests: &[(String, String)],
        outcome: CompositionOutcome,
        review_markdown: &OpaqueMarkdown,
        cited_source_refs: &[String],
    ) -> StoreResult<ToolReceiptView> {
        validate_digest("tool arguments digest", arguments_sha256)?;
        let now = now_unix();
        let transaction = self.immediate()?;
        let attempt = read_attempt(&transaction, attempt_id)?;
        require_attempt_kind(&attempt, AttemptKind::CompositionReview)?;
        if let Some(existing) =
            read_tool_receipt_optional(&transaction, &attempt.nucleus_job_id, call_id)?
        {
            if existing.arguments_sha256 != arguments_sha256 {
                return Err(StoreError::Conflict(
                    "tool call was replayed with different arguments".into(),
                ));
            }
            transaction.commit()?;
            return Ok(existing);
        }
        if !attempt.active {
            return Err(StoreError::Conflict(format!(
                "attempt {attempt_id} is no longer active"
            )));
        }
        require_prior_source_refs(&transaction, attempt_id, cited_source_refs)?;
        let (_, expected_agreements, _) = composition_basis(&transaction, &attempt.subject_id)?;
        let expected_basis_ids: BTreeSet<&str> = expected_agreements
            .iter()
            .map(|reference| reference.basis_id.as_str())
            .collect();
        let supplied_basis_ids: BTreeSet<&str> = observed_basis_digests
            .iter()
            .map(|(basis_id, _)| basis_id.as_str())
            .collect();
        if expected_basis_ids != supplied_basis_ids {
            return Err(StoreError::Conflict(
                "composition verification did not cover the exact active agreement bases".into(),
            ));
        }
        for (basis_id, observed_digest) in observed_basis_digests {
            let basis = read_basis(&transaction, basis_id)?;
            require_basis_digest(&basis, observed_digest)?;
            insert_basis_verification(&transaction, basis_id, Some(observed_digest), None)?;
        }
        let expected = attempt.expected_roster_digest.as_deref().ok_or_else(|| {
            StoreError::CorruptState("composition attempt has no expected digest".into())
        })?;
        let review_id = insert_composition_review(
            &transaction,
            &attempt.subject_id,
            expected,
            outcome,
            review_markdown,
            Some(attempt_id),
            now,
        )?;
        let result_json = terminal_result_json("composition_review", &review_id, outcome.as_str())?;
        bind_attempt_domain_result(
            &transaction,
            attempt_id,
            "composition_review",
            &review_id,
            now,
        )?;
        let receipt = insert_tool_receipt(
            &transaction,
            &attempt,
            call_id,
            arguments_sha256,
            &result_json,
            false,
            cited_source_refs,
            Some(("composition_review", &review_id)),
        )?;
        transaction.commit()?;
        Ok(receipt)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_conformance_tool_response(
        &mut self,
        attempt_id: &str,
        call_id: &str,
        arguments_sha256: &str,
        observed_candidate_basis_sha256: &str,
        outcome: ConformanceOutcome,
        review_markdown: &OpaqueMarkdown,
        cited_source_refs: &[String],
    ) -> StoreResult<ToolReceiptView> {
        validate_digest("tool arguments digest", arguments_sha256)?;
        validate_digest(
            "observed candidate basis digest",
            observed_candidate_basis_sha256,
        )?;
        let now = now_unix();
        let transaction = self.immediate()?;
        let attempt = read_attempt(&transaction, attempt_id)?;
        require_attempt_kind(&attempt, AttemptKind::ConformanceReview)?;
        if let Some(existing) =
            read_tool_receipt_optional(&transaction, &attempt.nucleus_job_id, call_id)?
        {
            if existing.arguments_sha256 != arguments_sha256 {
                return Err(StoreError::Conflict(
                    "tool call was replayed with different arguments".into(),
                ));
            }
            transaction.commit()?;
            return Ok(existing);
        }
        if !attempt.active {
            return Err(StoreError::Conflict(format!(
                "attempt {attempt_id} is no longer active"
            )));
        }
        require_prior_source_refs(&transaction, attempt_id, cited_source_refs)?;
        let basis_id = attempt.basis_id.as_deref().ok_or_else(|| {
            StoreError::CorruptState("conformance attempt has no candidate basis".into())
        })?;
        if attempt.basis_digest != observed_candidate_basis_sha256 {
            return Err(StoreError::Stale(
                "candidate basis changed during conformance review".into(),
            ));
        }
        let basis = read_basis(&transaction, basis_id)?;
        if basis.kind != BasisKind::Candidate {
            return Err(StoreError::Conflict(
                "conformance attempt basis is not a candidate".into(),
            ));
        }
        require_basis_digest(&basis, observed_candidate_basis_sha256)?;
        insert_basis_verification(
            &transaction,
            basis_id,
            Some(observed_candidate_basis_sha256),
            None,
        )?;
        let review_id = new_id("cnf");
        transaction.execute(
            "INSERT INTO conformance_reviews (
                review_id, agreement_id, candidate_basis_id, outcome,
                review_markdown, review_sha256, attempt_id, recorded_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                review_id,
                attempt.subject_id,
                basis_id,
                outcome.as_str(),
                review_markdown.as_bytes(),
                review_markdown.sha256(),
                attempt_id,
                now,
            ],
        )?;
        bind_attempt_domain_result(
            &transaction,
            attempt_id,
            "conformance_review",
            &review_id,
            now,
        )?;
        let result_json = terminal_result_json("conformance_review", &review_id, outcome.as_str())?;
        let receipt = insert_tool_receipt(
            &transaction,
            &attempt,
            call_id,
            arguments_sha256,
            &result_json,
            false,
            cited_source_refs,
            Some(("conformance_review", &review_id)),
        )?;
        transaction.commit()?;
        Ok(receipt)
    }
}
