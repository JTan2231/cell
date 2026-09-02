use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{
    Connection, OpenFlags, OptionalExtension as _, Transaction, TransactionBehavior, params,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::capture::{format_episode_id, gap_ordinal, normalize, parse_episode_id};
use crate::error::{AppError, AppResult, Context as _};
use crate::model::{
    Capture, EpisodeListItem, EpisodeRelation, Outcome, OutcomeStatus, RelatedEpisode,
    RevisionView, SearchResult, Settlement, SettlementStatus, SourceAnchor, SourceRole,
};

const SCHEMA: &str = include_str!("../schema.sql");
pub const SCHEMA_VERSION: i64 = 1;
const REQUIRED_TABLES: &[&str] = &[
    "geste_meta",
    "episodes",
    "episode_revisions",
    "revision_seals",
    "actions",
    "lessons",
    "gaps",
    "settlements",
    "tags",
    "sources",
    "source_supports",
    "related_episodes",
];
const REQUIRED_INDEXES: &[&str] = &["episode_revisions_latest"];
const REQUIRED_TRIGGERS: &[&str] = &[
    "episodes_no_update",
    "episodes_no_delete",
    "episode_revisions_no_update",
    "episode_revisions_no_delete",
    "revision_seals_no_update",
    "revision_seals_no_delete",
    "actions_no_update",
    "actions_no_delete",
    "lessons_no_update",
    "lessons_no_delete",
    "gaps_no_update",
    "gaps_no_delete",
    "settlements_no_update",
    "settlements_no_delete",
    "tags_no_update",
    "tags_no_delete",
    "sources_no_update",
    "sources_no_delete",
    "source_supports_no_update",
    "source_supports_no_delete",
    "related_episodes_no_update",
    "related_episodes_no_delete",
    "actions_no_insert_when_sealed",
    "lessons_no_insert_when_sealed",
    "gaps_no_insert_when_sealed",
    "settlements_no_insert_when_sealed",
    "tags_no_insert_when_sealed",
    "sources_no_insert_when_sealed",
    "source_supports_no_insert_when_sealed",
    "related_episodes_no_insert_when_sealed",
];

type CoreRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
);

pub struct Store {
    connection: Connection,
    database_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitResult {
    pub created: bool,
    pub schema_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorResult {
    pub schema_version: i64,
    pub foreign_keys: &'static str,
    pub integrity: &'static str,
    pub permissions: &'static str,
}

impl Store {
    pub fn init(path: &Path) -> AppResult<InitResult> {
        ensure_state_directory(path, true)?;
        let existed = path_exists(path)?;
        if existed {
            inspect_private_file(path, true)?;
            inspect_sidecars(path)?;
        } else {
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .context(
                    "database_create_failed",
                    format!("unable to create {}", path.display()),
                )?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).context(
                "database_create_failed",
                format!("unable to make {} private", path.display()),
            )?;
        }

        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .context(
            "database_open_failed",
            format!("unable to open {}", path.display()),
        )?;
        configure_connection(&connection)?;
        if existed {
            require_schema(&connection)?;
        } else {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .context(
                    "database_schema_failed",
                    "unable to begin Geste schema transaction",
                )?;
            transaction.execute_batch(SCHEMA).context(
                "database_schema_failed",
                "unable to initialize Geste schema",
            )?;
            transaction
                .commit()
                .context("database_schema_failed", "unable to commit Geste schema")?;
        }
        enable_wal(&connection)?;
        secure_sidecars(path)?;
        Ok(InitResult {
            created: !existed,
            schema_version: SCHEMA_VERSION,
        })
    }

    pub fn open_write(path: &Path) -> AppResult<Self> {
        ensure_state_directory(path, false)?;
        inspect_private_file(path, true)?;
        inspect_sidecars(path)?;
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .context(
            "database_open_failed",
            format!("unable to open {}", path.display()),
        )?;
        configure_connection(&connection)?;
        require_schema(&connection)?;
        enable_wal(&connection)?;
        secure_sidecars(path)?;
        Ok(Self {
            connection,
            database_path: path.to_path_buf(),
        })
    }

    pub fn open_read(path: &Path) -> AppResult<Self> {
        ensure_state_directory(path, false)?;
        inspect_private_file(path, true)?;
        inspect_sidecars(path)?;
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .context(
            "database_open_failed",
            format!("unable to open {}", path.display()),
        )?;
        configure_connection(&connection)?;
        require_schema(&connection)?;
        Ok(Self {
            connection,
            database_path: path.to_path_buf(),
        })
    }

    pub fn doctor(path: &Path) -> AppResult<DoctorResult> {
        let store = Self::open_read(path)?;
        require_all_revisions_sealed(&store.connection)?;
        let mut foreign_keys = store
            .connection
            .prepare("PRAGMA foreign_key_check")
            .context(
                "foreign_key_check_failed",
                "unable to run SQLite foreign_key_check",
            )?;
        let mut violations = foreign_keys.query([]).context(
            "foreign_key_check_failed",
            "unable to read SQLite foreign_key_check",
        )?;
        if violations
            .next()
            .context(
                "foreign_key_check_failed",
                "unable to read SQLite foreign_key_check result",
            )?
            .is_some()
        {
            return Err(AppError::new(
                "foreign_key_check_failed",
                "SQLite foreign_key_check reported a violation",
            ));
        }
        drop(violations);
        drop(foreign_keys);

        let integrity: String = store
            .connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .context(
                "integrity_check_failed",
                "unable to run SQLite integrity_check",
            )?;
        if integrity != "ok" {
            return Err(AppError::new(
                "integrity_check_failed",
                format!("SQLite integrity_check reported: {integrity}"),
            ));
        }
        inspect_private_file(path, true)?;
        inspect_sidecars(path)?;
        ensure_state_directory(path, false)?;
        Ok(DoctorResult {
            schema_version: SCHEMA_VERSION,
            foreign_keys: "ok",
            integrity: "ok",
            permissions: "private",
        })
    }

    pub fn create_episode(
        &mut self,
        capture: &Capture,
        submitted_sha256: &str,
    ) -> AppResult<RevisionView> {
        let recorded_at = now_rfc3339()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context(
                "database_write_failed",
                "unable to begin episode creation transaction",
            )?;
        require_all_revisions_sealed(&transaction)?;
        transaction
            .execute(
                "INSERT INTO episodes(created_at) VALUES(?1)",
                [&recorded_at],
            )
            .context("database_write_failed", "unable to create episode identity")?;
        let episode_id = transaction.last_insert_rowid();
        validate_related(&transaction, episode_id, capture)?;
        insert_revision(
            &transaction,
            episode_id,
            1,
            capture,
            submitted_sha256,
            &recorded_at,
        )?;
        transaction
            .commit()
            .context("database_write_failed", "unable to commit episode creation")?;
        secure_sidecars(&self.database_path)?;
        self.load_revision_by_id(episode_id, Some(1))
    }

    pub fn revise_episode(
        &mut self,
        episode: &str,
        base: u32,
        capture: &Capture,
        submitted_sha256: &str,
    ) -> AppResult<RevisionView> {
        if base == 0 {
            return Err(AppError::usage(
                "invalid_base_revision",
                "--base must be at least 1",
            ));
        }
        let episode_id = parse_episode_id(episode)?;
        let recorded_at = now_rfc3339()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .context(
                "database_write_failed",
                "unable to begin episode revision transaction",
            )?;
        require_all_revisions_sealed(&transaction)?;
        let head: Option<i64> = transaction
            .query_row(
                "SELECT MAX(revision) FROM episode_revisions WHERE episode_id = ?1",
                [episode_id],
                |row| row.get(0),
            )
            .context(
                "database_read_failed",
                "unable to read current episode revision",
            )?;
        let Some(head) = head else {
            return Err(AppError::usage(
                "episode_not_found",
                format!("episode {episode} does not exist"),
            ));
        };
        if i64::from(base) != head {
            return Err(AppError::new(
                "stale_revision",
                format!("episode {episode} is at revision {head}; supplied base was {base}"),
            ));
        }
        validate_related(&transaction, episode_id, capture)?;
        let revision = base.checked_add(1).ok_or_else(|| {
            AppError::new(
                "revision_overflow",
                format!("episode {episode} cannot append another revision"),
            )
        })?;
        insert_revision(
            &transaction,
            episode_id,
            revision,
            capture,
            submitted_sha256,
            &recorded_at,
        )?;
        transaction
            .commit()
            .context("database_write_failed", "unable to commit episode revision")?;
        secure_sidecars(&self.database_path)?;
        self.load_revision_by_id(episode_id, Some(revision))
    }

    pub fn load_revision(&self, episode: &str, at: Option<u32>) -> AppResult<RevisionView> {
        require_all_revisions_sealed(&self.connection)?;
        let episode_id = parse_episode_id(episode)?;
        self.load_revision_by_id(episode_id, at)
    }

    pub fn list(&self, limit: usize) -> AppResult<Vec<EpisodeListItem>> {
        require_all_revisions_sealed(&self.connection)?;
        let mut statement = self
            .connection
            .prepare(
                "SELECT r.episode_id, r.revision, r.title, r.shape,
                        r.outcome_status, r.basis_cutoff_at
                 FROM episode_revisions r
                 JOIN (
                    SELECT episode_id, MAX(revision) AS revision
                    FROM episode_revisions GROUP BY episode_id
                 ) head ON head.episode_id = r.episode_id AND head.revision = r.revision
                 ORDER BY r.episode_id ASC LIMIT ?1",
            )
            .context("database_read_failed", "unable to prepare episode list")?;
        let rows = statement
            .query_map([usize_to_i64(limit)?], |row| {
                let status: String = row.get(4)?;
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    status,
                    row.get::<_, String>(5)?,
                ))
            })
            .context("database_read_failed", "unable to read episode list")?;
        let mut items = Vec::new();
        for row in rows {
            let (id, revision, title, shape, status, cutoff) =
                row.context("database_read_failed", "unable to decode episode list")?;
            items.push(EpisodeListItem {
                episode: format_episode_id(id),
                revision,
                title,
                shape,
                outcome_status: decode_outcome(&status)?,
                basis_cutoff_at: cutoff,
            });
        }
        Ok(items)
    }

    pub fn search(&self, terms: &[String], limit: usize) -> AppResult<Vec<SearchResult>> {
        require_all_revisions_sealed(&self.connection)?;
        let heads = self.latest_revisions()?;
        let mut results = Vec::new();
        for revision in heads {
            if let Some(result) = score_revision(&revision, terms) {
                results.push(result);
            }
        }
        results.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.episode.len().cmp(&right.episode.len()))
                .then_with(|| left.episode.cmp(&right.episode))
        });
        results.truncate(limit);
        Ok(results)
    }

    fn latest_revisions(&self) -> AppResult<Vec<RevisionView>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT episode_id, MAX(revision) FROM episode_revisions
                 GROUP BY episode_id ORDER BY episode_id ASC",
            )
            .context(
                "database_read_failed",
                "unable to prepare latest episode revisions",
            )?;
        let rows = statement
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, u32>(1)?)))
            .context(
                "database_read_failed",
                "unable to read latest episode revisions",
            )?;
        let mut identities = Vec::new();
        for row in rows {
            identities.push(row.context(
                "database_read_failed",
                "unable to decode latest episode revision",
            )?);
        }
        drop(statement);
        identities
            .into_iter()
            .map(|(episode_id, revision)| self.load_revision_by_id(episode_id, Some(revision)))
            .collect()
    }

    #[allow(clippy::too_many_lines)]
    fn load_revision_by_id(&self, episode_id: i64, at: Option<u32>) -> AppResult<RevisionView> {
        let selected = match at {
            Some(revision) => {
                if revision == 0 {
                    return Err(AppError::usage(
                        "invalid_revision",
                        "episode revision must be at least 1",
                    ));
                }
                Some(revision)
            }
            None => self
                .connection
                .query_row(
                    "SELECT MAX(revision) FROM episode_revisions WHERE episode_id = ?1",
                    [episode_id],
                    |row| row.get::<_, Option<u32>>(0),
                )
                .context(
                    "database_read_failed",
                    "unable to select latest episode revision",
                )?,
        };
        let Some(revision) = selected else {
            return Err(AppError::usage(
                "episode_not_found",
                format!("episode {} does not exist", format_episode_id(episode_id)),
            ));
        };

        let core: Option<CoreRow> = self
            .connection
            .query_row(
                "SELECT submitted_sha256, recorded_at, title, shape, basis_cutoff_at,
                        recorded_by, situation, response, outcome_status,
                        outcome_summary, applicability
                 FROM episode_revisions WHERE episode_id = ?1 AND revision = ?2",
                params![episode_id, revision],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                    ))
                },
            )
            .optional()
            .context("database_read_failed", "unable to read episode revision")?;
        let Some((
            submitted_sha256,
            recorded_at,
            title,
            shape,
            basis_cutoff_at,
            recorded_by,
            situation,
            response,
            outcome_status,
            outcome_summary,
            applicability,
        )) = core
        else {
            let exists: bool = self
                .connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM episodes WHERE id = ?1)",
                    [episode_id],
                    |row| row.get(0),
                )
                .context("database_read_failed", "unable to inspect episode identity")?;
            let (error_code, message) = if exists {
                (
                    "revision_not_found",
                    format!(
                        "episode {} has no revision {revision}",
                        format_episode_id(episode_id)
                    ),
                )
            } else {
                (
                    "episode_not_found",
                    format!("episode {} does not exist", format_episode_id(episode_id)),
                )
            };
            return Err(AppError::usage(error_code, message));
        };

        let actions = load_ordered_values(&self.connection, "actions", episode_id, revision)?;
        let lessons = load_ordered_values(&self.connection, "lessons", episode_id, revision)?;
        let gaps = load_ordered_values(&self.connection, "gaps", episode_id, revision)?;
        let tags = load_ordered_values(&self.connection, "tags", episode_id, revision)?;
        let settlements = load_settlements(&self.connection, episode_id, revision)?;
        let sources = load_sources(&self.connection, episode_id, revision)?;
        let related_episodes = load_related(&self.connection, episode_id, revision)?;
        Ok(RevisionView {
            episode: format_episode_id(episode_id),
            revision,
            submitted_sha256,
            recorded_at,
            capture: Capture {
                schema_version: 1,
                title,
                shape,
                basis_cutoff_at,
                recorded_by,
                situation,
                response,
                outcome: Outcome {
                    status: decode_outcome(&outcome_status)?,
                    summary: outcome_summary,
                },
                applicability,
                actions,
                lessons,
                settlements,
                tags,
                gaps,
                sources,
                related_episodes,
            },
        })
    }
}

#[allow(clippy::too_many_lines)]
fn insert_revision(
    transaction: &Transaction<'_>,
    episode_id: i64,
    revision: u32,
    capture: &Capture,
    submitted_sha256: &str,
    recorded_at: &str,
) -> AppResult<()> {
    transaction
        .execute(
            "INSERT INTO episode_revisions(
                episode_id, revision, submitted_sha256, recorded_at, title, shape,
                basis_cutoff_at, recorded_by, situation, response, outcome_status,
                outcome_summary, applicability
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                episode_id,
                revision,
                submitted_sha256,
                recorded_at,
                capture.title,
                capture.shape,
                capture.basis_cutoff_at,
                capture.recorded_by,
                capture.situation,
                capture.response,
                capture.outcome.status.as_str(),
                capture.outcome.summary,
                capture.applicability,
            ],
        )
        .context("database_write_failed", "unable to insert episode revision")?;

    insert_ordered_values(
        transaction,
        "actions",
        episode_id,
        revision,
        &capture.actions,
    )?;
    insert_ordered_values(
        transaction,
        "lessons",
        episode_id,
        revision,
        &capture.lessons,
    )?;
    insert_ordered_values(transaction, "gaps", episode_id, revision, &capture.gaps)?;
    for (index, tag) in capture.tags.iter().enumerate() {
        let ordinal = collection_ordinal(index)?;
        transaction
            .execute(
                "INSERT INTO tags(episode_id, revision, ordinal, value, normalized)
                 VALUES(?1, ?2, ?3, ?4, ?5)",
                params![episode_id, revision, ordinal, tag, normalize(tag)],
            )
            .context("database_write_failed", "unable to insert episode tag")?;
    }
    for settlement in &capture.settlements {
        let gap = settlement.gap.as_deref().map(gap_ordinal).transpose()?;
        transaction
            .execute(
                "INSERT INTO settlements(
                    episode_id, revision, settlement_id, statement, status, gap_ordinal
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    episode_id,
                    revision,
                    settlement.id,
                    settlement.statement,
                    settlement.status.as_str(),
                    gap,
                ],
            )
            .context(
                "database_write_failed",
                "unable to insert episode settlement",
            )?;
    }
    for source in &capture.sources {
        transaction
            .execute(
                "INSERT INTO sources(
                    episode_id, revision, source_id, system, kind, reference,
                    source_revision, digest, observed_at, role, label
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    episode_id,
                    revision,
                    source.id,
                    source.system,
                    source.kind,
                    source.reference,
                    source.revision,
                    source.digest,
                    source.observed_at,
                    source.role.as_str(),
                    source.label,
                ],
            )
            .context("database_write_failed", "unable to insert source anchor")?;
        for target in &source.supports {
            transaction
                .execute(
                    "INSERT INTO source_supports(
                        episode_id, revision, source_id, target
                     ) VALUES(?1, ?2, ?3, ?4)",
                    params![episode_id, revision, source.id, target],
                )
                .context("database_write_failed", "unable to insert source support")?;
        }
    }
    for (index, link) in capture.related_episodes.iter().enumerate() {
        let ordinal = collection_ordinal(index)?;
        transaction
            .execute(
                "INSERT INTO related_episodes(
                    episode_id, revision, ordinal, related_episode_id,
                    related_revision, relation
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    episode_id,
                    revision,
                    ordinal,
                    parse_episode_id(&link.episode)?,
                    link.revision,
                    link.relation.as_str(),
                ],
            )
            .context("database_write_failed", "unable to insert related episode")?;
    }
    transaction
        .execute(
            "INSERT INTO revision_seals(episode_id, revision, sealed_at)
             VALUES(?1, ?2, ?3)",
            params![episode_id, revision, recorded_at],
        )
        .context("database_write_failed", "unable to seal episode revision")?;
    Ok(())
}

fn insert_ordered_values(
    transaction: &Transaction<'_>,
    table: &str,
    episode_id: i64,
    revision: u32,
    values: &[String],
) -> AppResult<()> {
    let sql =
        format!("INSERT INTO {table}(episode_id, revision, ordinal, value) VALUES(?1, ?2, ?3, ?4)");
    for (index, value) in values.iter().enumerate() {
        let ordinal = collection_ordinal(index)?;
        transaction
            .execute(&sql, params![episode_id, revision, ordinal, value])
            .context(
                "database_write_failed",
                format!("unable to insert episode {table}"),
            )?;
    }
    Ok(())
}

fn validate_related(
    transaction: &Transaction<'_>,
    episode_id: i64,
    capture: &Capture,
) -> AppResult<()> {
    for link in &capture.related_episodes {
        let related = parse_episode_id(&link.episode)?;
        if related == episode_id {
            return Err(AppError::usage(
                "self_related_episode",
                format!(
                    "episode {} cannot relate to itself",
                    format_episode_id(episode_id)
                ),
            ));
        }
        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM episode_revisions
                    WHERE episode_id = ?1 AND revision = ?2
                 )",
                params![related, link.revision],
                |row| row.get(0),
            )
            .context("database_read_failed", "unable to validate related episode")?;
        if !exists {
            return Err(AppError::usage(
                "related_episode_not_found",
                format!(
                    "related episode {} revision {} does not exist",
                    link.episode, link.revision
                ),
            ));
        }
    }
    Ok(())
}

fn load_ordered_values(
    connection: &Connection,
    table: &str,
    episode_id: i64,
    revision: u32,
) -> AppResult<Vec<String>> {
    let sql = format!(
        "SELECT value FROM {table} WHERE episode_id = ?1 AND revision = ?2 ORDER BY ordinal ASC"
    );
    let mut statement = connection.prepare(&sql).context(
        "database_read_failed",
        format!("unable to prepare episode {table}"),
    )?;
    let rows = statement
        .query_map(params![episode_id, revision], |row| row.get(0))
        .context(
            "database_read_failed",
            format!("unable to read episode {table}"),
        )?;
    rows.map(|row| {
        row.context(
            "database_read_failed",
            format!("unable to decode episode {table}"),
        )
    })
    .collect()
}

fn load_settlements(
    connection: &Connection,
    episode_id: i64,
    revision: u32,
) -> AppResult<Vec<Settlement>> {
    let mut statement = connection
        .prepare(
            "SELECT settlement_id, statement, status, gap_ordinal
             FROM settlements WHERE episode_id = ?1 AND revision = ?2
             ORDER BY rowid ASC",
        )
        .context("database_read_failed", "unable to prepare settlements")?;
    let rows = statement
        .query_map(params![episode_id, revision], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })
        .context("database_read_failed", "unable to read settlements")?;
    let mut settlements = Vec::new();
    for row in rows {
        let (id, statement, status, gap) =
            row.context("database_read_failed", "unable to decode settlement")?;
        settlements.push(Settlement {
            id,
            statement,
            status: SettlementStatus::parse(&status).ok_or_else(|| {
                AppError::new(
                    "database_state_invalid",
                    format!("stored settlement status {status:?} is invalid"),
                )
            })?,
            gap: gap.map(|ordinal| format!("gap:{ordinal}")),
        });
    }
    Ok(settlements)
}

fn load_sources(
    connection: &Connection,
    episode_id: i64,
    revision: u32,
) -> AppResult<Vec<SourceAnchor>> {
    type SourceRow = (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
    );
    let mut statement = connection
        .prepare(
            "SELECT source_id, system, kind, reference, source_revision, digest,
                    observed_at, role, label
             FROM sources WHERE episode_id = ?1 AND revision = ?2 ORDER BY rowid ASC",
        )
        .context("database_read_failed", "unable to prepare source anchors")?;
    let rows = statement
        .query_map(params![episode_id, revision], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ))
        })
        .context("database_read_failed", "unable to read source anchors")?;
    let mut source_rows: Vec<SourceRow> = Vec::new();
    for row in rows {
        source_rows.push(row.context("database_read_failed", "unable to decode source anchor")?);
    }
    drop(statement);
    let mut sources = Vec::new();
    for (id, system, kind, reference, source_revision, digest, observed_at, role, label) in
        source_rows
    {
        let mut supports_statement = connection
            .prepare(
                "SELECT target FROM source_supports
                 WHERE episode_id = ?1 AND revision = ?2 AND source_id = ?3
                 ORDER BY rowid ASC",
            )
            .context("database_read_failed", "unable to prepare source supports")?;
        let supports_rows = supports_statement
            .query_map(params![episode_id, revision, id], |row| row.get(0))
            .context("database_read_failed", "unable to read source supports")?;
        let supports = supports_rows
            .map(|row| row.context("database_read_failed", "unable to decode source support"))
            .collect::<AppResult<Vec<String>>>()?;
        sources.push(SourceAnchor {
            id,
            system,
            kind,
            reference,
            revision: source_revision,
            digest,
            observed_at,
            role: SourceRole::parse(&role).ok_or_else(|| {
                AppError::new(
                    "database_state_invalid",
                    format!("stored source role {role:?} is invalid"),
                )
            })?,
            label,
            supports,
        });
    }
    Ok(sources)
}

fn load_related(
    connection: &Connection,
    episode_id: i64,
    revision: u32,
) -> AppResult<Vec<RelatedEpisode>> {
    let mut statement = connection
        .prepare(
            "SELECT related_episode_id, related_revision, relation
             FROM related_episodes WHERE episode_id = ?1 AND revision = ?2
             ORDER BY ordinal ASC",
        )
        .context("database_read_failed", "unable to prepare related episodes")?;
    let rows = statement
        .query_map(params![episode_id, revision], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .context("database_read_failed", "unable to read related episodes")?;
    let mut links = Vec::new();
    for row in rows {
        let (related, related_revision, relation) =
            row.context("database_read_failed", "unable to decode related episode")?;
        links.push(RelatedEpisode {
            episode: format_episode_id(related),
            revision: related_revision,
            relation: EpisodeRelation::parse(&relation).ok_or_else(|| {
                AppError::new(
                    "database_state_invalid",
                    format!("stored episode relation {relation:?} is invalid"),
                )
            })?,
        });
    }
    Ok(links)
}

fn score_revision(revision: &RevisionView, terms: &[String]) -> Option<SearchResult> {
    let capture = &revision.capture;
    let tags: Vec<String> = capture.tags.iter().map(|tag| normalize(tag)).collect();
    let shape = normalize(&capture.shape);
    let title = normalize(&capture.title);
    let situation = normalize(&capture.situation);
    let response = normalize(&capture.response);
    let applicability = normalize(&capture.applicability);
    let outcome = normalize(&capture.outcome.summary);
    let outcome_status = capture.outcome.status.as_str();
    let lessons: Vec<String> = capture
        .lessons
        .iter()
        .map(|lesson| normalize(lesson))
        .collect();
    let mut score = 0;
    let mut matched_fields = BTreeSet::new();
    for term in terms {
        let mut best = 0;
        if tags.iter().any(|tag| tag == term) {
            best = best.max(8);
            matched_fields.insert("tag".to_owned());
        }
        for (field, value, weight) in [
            ("shape", shape.as_str(), 6),
            ("title", title.as_str(), 5),
            ("situation", situation.as_str(), 3),
            ("response", response.as_str(), 3),
            ("applicability", applicability.as_str(), 3),
        ] {
            if value.contains(term) {
                best = best.max(weight);
                matched_fields.insert(field.to_owned());
            }
        }
        if outcome.contains(term) || outcome_status.contains(term) {
            best = best.max(2);
            matched_fields.insert("outcome".to_owned());
        }
        if lessons.iter().any(|lesson| lesson.contains(term)) {
            best = best.max(2);
            matched_fields.insert("lesson".to_owned());
        }
        if best == 0 {
            return None;
        }
        score += best;
    }
    Some(SearchResult {
        episode: revision.episode.clone(),
        revision: revision.revision,
        title: capture.title.clone(),
        shape: capture.shape.clone(),
        outcome_status: capture.outcome.status,
        score,
        matched_terms: terms.to_vec(),
        matched_fields: matched_fields.into_iter().collect(),
    })
}

fn decode_outcome(value: &str) -> AppResult<OutcomeStatus> {
    OutcomeStatus::parse(value).ok_or_else(|| {
        AppError::new(
            "database_state_invalid",
            format!("stored outcome status {value:?} is invalid"),
        )
    })
}

fn configure_connection(connection: &Connection) -> AppResult<()> {
    connection.busy_timeout(Duration::from_secs(5)).context(
        "database_config_failed",
        "unable to set SQLite busy timeout",
    )?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .context("database_config_failed", "unable to enable foreign keys")
}

fn enable_wal(connection: &Connection) -> AppResult<()> {
    let mode: String = connection
        .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
        .context("database_config_failed", "unable to enable SQLite WAL")?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(AppError::new(
            "database_config_failed",
            format!("SQLite did not enable WAL mode; reported {mode:?}"),
        ));
    }
    Ok(())
}

fn require_schema(connection: &Connection) -> AppResult<()> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .context(
            "database_schema_failed",
            "unable to inspect database schema version",
        )?;
    let marker_version = connection
        .query_row(
            "SELECT schema_version FROM geste_meta WHERE marker = 'geste'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional();
    let marker_version = match marker_version {
        Ok(value) => value,
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table") =>
        {
            None
        }
        Err(error) => {
            return Err(AppError::new(
                "database_schema_failed",
                format!("unable to inspect Geste schema marker: {error}"),
            ));
        }
    };
    let Some(marker_version) = marker_version else {
        return Err(AppError::new(
            "not_geste_database",
            "existing database is not a Geste database",
        ));
    };
    if version != SCHEMA_VERSION || marker_version != SCHEMA_VERSION {
        return Err(AppError::new(
            "unsupported_schema",
            format!(
                "Geste schema {version}/{marker_version} is unsupported; expected schema {SCHEMA_VERSION}"
            ),
        ));
    }
    for name in REQUIRED_TABLES {
        require_schema_object(connection, "table", name)?;
    }
    for name in REQUIRED_INDEXES {
        require_schema_object(connection, "index", name)?;
    }
    for name in REQUIRED_TRIGGERS {
        require_schema_object(connection, "trigger", name)?;
    }
    require_all_revisions_sealed(connection)?;
    Ok(())
}

fn require_schema_object(connection: &Connection, kind: &str, name: &str) -> AppResult<()> {
    let actual: Option<String> = connection
        .query_row(
            "SELECT type FROM sqlite_master WHERE name = ?1",
            [name],
            |row| row.get(0),
        )
        .optional()
        .context(
            "database_schema_failed",
            "unable to inspect required Geste schema objects",
        )?;
    if actual.as_deref() != Some(kind) {
        return Err(AppError::new(
            "database_schema_incomplete",
            format!("Geste schema 1 is missing required {kind} {name}"),
        ));
    }
    Ok(())
}

fn require_all_revisions_sealed(connection: &Connection) -> AppResult<()> {
    let unsealed: Option<(i64, i64)> = connection
        .query_row(
            "SELECT r.episode_id, r.revision
             FROM episode_revisions r
             LEFT JOIN revision_seals s
               ON s.episode_id = r.episode_id AND s.revision = r.revision
             WHERE s.episode_id IS NULL
             ORDER BY r.episode_id, r.revision
             LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .context(
            "database_schema_failed",
            "unable to verify sealed episode revisions",
        )?;
    if let Some((episode_id, revision)) = unsealed {
        return Err(AppError::new(
            "unsealed_revision",
            format!(
                "episode {} revision {revision} is not sealed",
                format_episode_id(episode_id)
            ),
        ));
    }
    Ok(())
}

fn ensure_state_directory(database: &Path, create: bool) -> AppResult<()> {
    let parent = database
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let metadata = match fs::symlink_metadata(parent) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
            fs::create_dir_all(parent).context(
                "state_directory_failed",
                format!("unable to create {}", parent.display()),
            )?;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).context(
                "state_directory_failed",
                format!("unable to make {} private", parent.display()),
            )?;
            fs::symlink_metadata(parent).context(
                "state_directory_failed",
                format!("unable to inspect {}", parent.display()),
            )?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::new(
                "database_missing",
                format!("database state directory is missing: {}", parent.display()),
            ));
        }
        Err(error) => {
            return Err(AppError::new(
                "state_directory_failed",
                format!("unable to inspect {}: {error}", parent.display()),
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AppError::new(
            "unsafe_state_path",
            format!(
                "state directory must be a regular directory: {}",
                parent.display()
            ),
        ));
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o700 {
        return Err(AppError::new(
            "unsafe_permissions",
            format!(
                "state directory {} has mode {mode:04o}; expected 0700",
                parent.display()
            ),
        ));
    }
    Ok(())
}

fn inspect_private_file(path: &Path, required: bool) -> AppResult<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::new(
                "database_missing",
                format!("database does not exist: {}", path.display()),
            ));
        }
        Err(error) => {
            return Err(AppError::new(
                "database_open_failed",
                format!("unable to inspect {}: {error}", path.display()),
            ));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::new(
            "unsafe_state_path",
            format!("database state must be a regular file: {}", path.display()),
        ));
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(AppError::new(
            "unsafe_permissions",
            format!(
                "database state {} has mode {mode:04o}; expected 0600",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn inspect_sidecars(database: &Path) -> AppResult<()> {
    for sidecar in database_sidecars(database) {
        inspect_private_file(&sidecar, false)?;
    }
    Ok(())
}

fn secure_sidecars(database: &Path) -> AppResult<()> {
    for sidecar in database_sidecars(database) {
        match fs::symlink_metadata(&sidecar) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(AppError::new(
                        "unsafe_state_path",
                        format!(
                            "database state must be a regular file: {}",
                            sidecar.display()
                        ),
                    ));
                }
                fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o600)).context(
                    "database_open_failed",
                    format!("unable to make {} private", sidecar.display()),
                )?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(AppError::new(
                    "database_open_failed",
                    format!("unable to inspect {}: {error}", sidecar.display()),
                ));
            }
        }
    }
    Ok(())
}

fn database_sidecars(path: &Path) -> [PathBuf; 3] {
    ["-wal", "-shm", "-journal"].map(|suffix| {
        let mut value = OsString::from(path.as_os_str());
        value.push(suffix);
        PathBuf::from(value)
    })
}

fn path_exists(path: &Path) -> AppResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(AppError::new(
            "database_open_failed",
            format!("unable to inspect {}: {error}", path.display()),
        )),
    }
}

fn now_rfc3339() -> AppResult<String> {
    OffsetDateTime::now_utc().format(&Rfc3339).map_err(|error| {
        AppError::new(
            "clock_failed",
            format!("unable to format the current time: {error}"),
        )
    })
}

fn usize_to_i64(value: usize) -> AppResult<i64> {
    i64::try_from(value).map_err(|_| AppError::usage("invalid_limit", "limit is too large"))
}

fn collection_ordinal(index: usize) -> AppResult<i64> {
    index
        .checked_add(1)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| {
            AppError::new(
                "collection_ordinal_overflow",
                "capture collection ordinal exceeds SQLite range",
            )
        })
}
