use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, Transaction, params};

use crate::corpus::{CorpusEffect, CorpusState, SnapshotEvidence};
use crate::error::AppError;
use crate::index;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RevisionStats {
    pub concepts: i64,
    pub edges: i64,
    pub evidence: i64,
}

/// Append the canonical typed effects for a commit whose provenance row has
/// already been inserted in the same transaction.
#[allow(clippy::too_many_lines)]
pub(crate) fn insert_canonical_revision(
    transaction: &Transaction<'_>,
    revision: i64,
    expected: &CorpusState,
) -> Result<(), AppError> {
    if revision <= 0 {
        return Err(AppError::database(
            "invalid_commit_revision",
            "a committed revision must be positive",
        ));
    }
    let commit_exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM commits WHERE revision = ?1)",
        [revision],
        |row| row.get::<_, bool>(0),
    )?;
    if !commit_exists {
        return Err(AppError::database(
            "commit_missing",
            format!("cannot append effects for missing revision {revision}"),
        ));
    }
    let before = load_revision_snapshot(transaction, revision - 1)?.ok_or_else(|| {
        AppError::database(
            "commit_parent_missing",
            format!("revision {revision} has no replayable parent"),
        )
    })?;
    let effects = derive_effects(&before, expected);
    if effects.is_empty() {
        return Err(AppError::conflict(
            "empty_commit",
            "a commit must contain at least one corpus effect",
        ));
    }

    let mut concept_ordinal = 0_i64;
    let mut edge_ordinal = 0_i64;
    let mut evidence_ordinal = 0_i64;
    for effect in &effects {
        match effect {
            CorpusEffect::CreateConcept { concept_id, label } => {
                insert_concept_effect(
                    transaction,
                    revision,
                    concept_ordinal,
                    *concept_id,
                    "create",
                    Some(label),
                )?;
                concept_ordinal += 1;
            }
            CorpusEffect::RewordConcept { concept_id, label } => {
                insert_concept_effect(
                    transaction,
                    revision,
                    concept_ordinal,
                    *concept_id,
                    "reword",
                    Some(label),
                )?;
                concept_ordinal += 1;
            }
            CorpusEffect::RetireConcept { concept_id } => {
                insert_concept_effect(
                    transaction,
                    revision,
                    concept_ordinal,
                    *concept_id,
                    "retire",
                    None,
                )?;
                concept_ordinal += 1;
            }
            CorpusEffect::AddParent {
                parent_id,
                child_id,
            } => {
                insert_edge_effect(
                    transaction,
                    revision,
                    edge_ordinal,
                    *parent_id,
                    *child_id,
                    "add",
                )?;
                edge_ordinal += 1;
            }
            CorpusEffect::RemoveParent {
                parent_id,
                child_id,
            } => {
                insert_edge_effect(
                    transaction,
                    revision,
                    edge_ordinal,
                    *parent_id,
                    *child_id,
                    "remove",
                )?;
                edge_ordinal += 1;
            }
            CorpusEffect::AddEvidence(item) => {
                insert_evidence_effect(transaction, revision, evidence_ordinal, item, "add")?;
                evidence_ordinal += 1;
            }
            CorpusEffect::RemoveEvidence(item) => {
                insert_evidence_effect(transaction, revision, evidence_ordinal, item, "remove")?;
                evidence_ordinal += 1;
            }
        }
    }

    let persisted = load_revision_effects(transaction, revision)?;
    let replayed = before.reduced(&persisted)?;
    if replayed != *expected {
        return Err(AppError::database(
            "commit_effect_mismatch",
            format!("revision {revision} effects do not reproduce the requested state"),
        ));
    }
    Ok(())
}

pub(crate) fn revision_exists(connection: &Connection, revision: i64) -> Result<bool, AppError> {
    if revision == 0 {
        return Ok(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM library_identity WHERE singleton = 1)",
            [],
            |row| row.get(0),
        )?);
    }
    if revision < 0 {
        return Ok(false);
    }
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM commits WHERE revision = ?1)",
        [revision],
        |row| row.get(0),
    )?)
}

pub(crate) fn revision_stats(
    connection: &Connection,
    revision: i64,
) -> Result<Option<RevisionStats>, AppError> {
    let Some(state) = load_revision_snapshot(connection, revision)? else {
        return Ok(None);
    };
    Ok(Some(RevisionStats {
        concepts: i64::try_from(state.concepts.len()).map_err(|_| numeric_overflow())?,
        edges: i64::try_from(state.edges.len()).map_err(|_| numeric_overflow())?,
        evidence: i64::try_from(state.evidence.len()).map_err(|_| numeric_overflow())?,
    }))
}

/// Replay the immutable effect stream from revision zero through `revision`.
pub(crate) fn load_revision_snapshot(
    connection: &Connection,
    revision: i64,
) -> Result<Option<CorpusState>, AppError> {
    if revision < 0 || !revision_exists(connection, revision)? {
        return Ok(None);
    }
    let mut state = CorpusState::empty();
    if revision == 0 {
        return Ok(Some(state));
    }
    let mut statement = connection
        .prepare("SELECT revision FROM commits WHERE revision <= ?1 ORDER BY revision")?;
    let rows = statement.query_map([revision], |row| row.get::<_, i64>(0))?;
    let revisions = rows.collect::<Result<Vec<_>, _>>()?;
    let expected_count = usize::try_from(revision).map_err(|_| numeric_overflow())?;
    if revisions.len() != expected_count
        || revisions
            .iter()
            .enumerate()
            .any(|(index, stored)| *stored != i64::try_from(index + 1).unwrap_or(i64::MAX))
    {
        return Err(AppError::database(
            "invalid_commit_sequence",
            "the commit log is not a contiguous sequence beginning at revision 1",
        ));
    }
    for stored_revision in revisions {
        state = state.reduced(&load_revision_effects(connection, stored_revision)?)?;
    }
    Ok(Some(state))
}

/// Populate connection-local relational projections from the one replayed
/// state.  Graph SQL may page and walk these TEMP rows, but they disappear with
/// the connection and are never another source of truth.
pub(crate) fn prepare_graph_revision(
    connection: &Connection,
    revision: i64,
) -> Result<(), AppError> {
    connection.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS prepared_corpus_revisions(
             revision INTEGER PRIMARY KEY
         );
         CREATE TEMP TABLE IF NOT EXISTS revision_concepts(
             revision INTEGER NOT NULL,
             concept_id INTEGER NOT NULL,
             label TEXT NOT NULL,
             normalized_label TEXT NOT NULL,
             parent_count INTEGER NOT NULL,
             child_count INTEGER NOT NULL,
             evidence_count INTEGER NOT NULL,
             PRIMARY KEY(revision, concept_id)
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS revision_edges(
             revision INTEGER NOT NULL,
             parent_id INTEGER NOT NULL,
             child_id INTEGER NOT NULL,
             PRIMARY KEY(revision, parent_id, child_id)
         ) WITHOUT ROWID;
         CREATE TEMP TABLE IF NOT EXISTS revision_evidence(
             revision INTEGER NOT NULL,
             concept_id INTEGER NOT NULL,
             work_id INTEGER NOT NULL,
             start_byte INTEGER NOT NULL,
             end_byte INTEGER NOT NULL,
             PRIMARY KEY(revision, concept_id, work_id, start_byte, end_byte)
         ) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS temp.revision_concepts_by_label
             ON revision_concepts(revision, normalized_label, concept_id);
         CREATE INDEX IF NOT EXISTS temp.revision_edges_by_child
             ON revision_edges(revision, child_id, parent_id);
         CREATE INDEX IF NOT EXISTS temp.revision_evidence_by_work_range
             ON revision_evidence(revision, work_id, start_byte, end_byte, concept_id);",
    )?;
    if connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM temp.prepared_corpus_revisions WHERE revision = ?1
         )",
        [revision],
        |row| row.get::<_, bool>(0),
    )? {
        return Ok(());
    }
    let state = load_revision_snapshot(connection, revision)?
        .ok_or_else(|| revision_not_found(revision))?;

    let mut parent_counts = BTreeMap::<i64, i64>::new();
    let mut child_counts = BTreeMap::<i64, i64>::new();
    let mut evidence_counts = BTreeMap::<i64, i64>::new();
    for edge in &state.edges {
        *parent_counts.entry(edge.child_id).or_default() += 1;
        *child_counts.entry(edge.parent_id).or_default() += 1;
    }
    for evidence in &state.evidence {
        *evidence_counts.entry(evidence.concept_id).or_default() += 1;
    }
    for concept in &state.concepts {
        connection.execute(
            "INSERT INTO temp.revision_concepts(
                 revision, concept_id, label, normalized_label,
                 parent_count, child_count, evidence_count
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                revision,
                concept.id,
                concept.label,
                index::normalize(&concept.label),
                parent_counts.get(&concept.id).copied().unwrap_or(0),
                child_counts.get(&concept.id).copied().unwrap_or(0),
                evidence_counts.get(&concept.id).copied().unwrap_or(0),
            ],
        )?;
    }
    for edge in &state.edges {
        connection.execute(
            "INSERT INTO temp.revision_edges(revision, parent_id, child_id)
             VALUES(?1, ?2, ?3)",
            params![revision, edge.parent_id, edge.child_id],
        )?;
    }
    for evidence in &state.evidence {
        connection.execute(
            "INSERT INTO temp.revision_evidence(
                 revision, concept_id, work_id, start_byte, end_byte
             ) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                revision,
                evidence.concept_id,
                evidence.work_id,
                i64::try_from(evidence.start_byte).map_err(|_| numeric_overflow())?,
                i64::try_from(evidence.end_byte).map_err(|_| numeric_overflow())?,
            ],
        )?;
    }
    connection.execute(
        "INSERT INTO temp.prepared_corpus_revisions(revision) VALUES(?1)",
        [revision],
    )?;
    Ok(())
}

pub(crate) fn load_revision_effects(
    connection: &Connection,
    revision: i64,
) -> Result<Vec<CorpusEffect>, AppError> {
    let mut effects = Vec::new();
    let mut concepts = connection.prepare(
        "SELECT concept_id, effect, label
         FROM concept_effects WHERE revision = ?1 ORDER BY ordinal",
    )?;
    for row in concepts.query_map([revision], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })? {
        let (concept_id, effect, label) = row?;
        effects.push(match (effect.as_str(), label) {
            ("create", Some(label)) => CorpusEffect::CreateConcept { concept_id, label },
            ("reword", Some(label)) => CorpusEffect::RewordConcept { concept_id, label },
            ("retire", None) => CorpusEffect::RetireConcept { concept_id },
            _ => return Err(invalid_effect_row(revision, "concept")),
        });
    }

    let mut edges = connection.prepare(
        "SELECT parent_id, child_id, effect
         FROM parent_edge_effects WHERE revision = ?1 ORDER BY ordinal",
    )?;
    for row in edges.query_map([revision], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })? {
        let (parent_id, child_id, effect) = row?;
        effects.push(match effect.as_str() {
            "add" => CorpusEffect::AddParent {
                parent_id,
                child_id,
            },
            "remove" => CorpusEffect::RemoveParent {
                parent_id,
                child_id,
            },
            _ => return Err(invalid_effect_row(revision, "parent edge")),
        });
    }

    let mut evidence = connection.prepare(
        "SELECT concept_id, work_id, start_byte, end_byte, effect
         FROM evidence_link_effects WHERE revision = ?1 ORDER BY ordinal",
    )?;
    for row in evidence.query_map([revision], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
        ))
    })? {
        let (concept_id, work_id, start_byte, end_byte, effect) = row?;
        let item = SnapshotEvidence {
            concept_id,
            work_id,
            start_byte: usize::try_from(start_byte).map_err(|_| numeric_overflow())?,
            end_byte: usize::try_from(end_byte).map_err(|_| numeric_overflow())?,
        };
        effects.push(match effect.as_str() {
            "add" => CorpusEffect::AddEvidence(item),
            "remove" => CorpusEffect::RemoveEvidence(item),
            _ => return Err(invalid_effect_row(revision, "evidence link")),
        });
    }
    Ok(effects)
}

pub(crate) fn derive_effects(before: &CorpusState, after: &CorpusState) -> Vec<CorpusEffect> {
    let before_concepts = before
        .concepts
        .iter()
        .map(|concept| (concept.id, concept))
        .collect::<BTreeMap<_, _>>();
    let after_concepts = after
        .concepts
        .iter()
        .map(|concept| (concept.id, concept))
        .collect::<BTreeMap<_, _>>();
    let mut effects = Vec::new();
    for id in before_concepts
        .keys()
        .chain(after_concepts.keys())
        .copied()
        .collect::<BTreeSet<_>>()
    {
        match (before_concepts.get(&id), after_concepts.get(&id)) {
            (None, Some(concept)) => effects.push(CorpusEffect::CreateConcept {
                concept_id: id,
                label: concept.label.clone(),
            }),
            (Some(_), None) => effects.push(CorpusEffect::RetireConcept { concept_id: id }),
            (Some(old), Some(new)) if old.label != new.label => {
                effects.push(CorpusEffect::RewordConcept {
                    concept_id: id,
                    label: new.label.clone(),
                });
            }
            _ => {}
        }
    }

    let before_edges = before.edges.iter().cloned().collect::<BTreeSet<_>>();
    let after_edges = after.edges.iter().cloned().collect::<BTreeSet<_>>();
    for edge in before_edges.difference(&after_edges) {
        effects.push(CorpusEffect::RemoveParent {
            parent_id: edge.parent_id,
            child_id: edge.child_id,
        });
    }
    for edge in after_edges.difference(&before_edges) {
        effects.push(CorpusEffect::AddParent {
            parent_id: edge.parent_id,
            child_id: edge.child_id,
        });
    }

    let before_evidence = before.evidence.iter().cloned().collect::<BTreeSet<_>>();
    let after_evidence = after.evidence.iter().cloned().collect::<BTreeSet<_>>();
    for item in before_evidence.difference(&after_evidence) {
        effects.push(CorpusEffect::RemoveEvidence(item.clone()));
    }
    for item in after_evidence.difference(&before_evidence) {
        effects.push(CorpusEffect::AddEvidence(item.clone()));
    }
    effects
}

fn insert_concept_effect(
    transaction: &Transaction<'_>,
    revision: i64,
    ordinal: i64,
    concept_id: i64,
    effect: &str,
    label: Option<&String>,
) -> Result<(), AppError> {
    transaction.execute(
        "INSERT INTO concept_effects(revision, ordinal, concept_id, effect, label)
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![revision, ordinal, concept_id, effect, label],
    )?;
    Ok(())
}

fn insert_edge_effect(
    transaction: &Transaction<'_>,
    revision: i64,
    ordinal: i64,
    parent_id: i64,
    child_id: i64,
    effect: &str,
) -> Result<(), AppError> {
    transaction.execute(
        "INSERT INTO parent_edge_effects(
             revision, ordinal, parent_id, child_id, effect
         ) VALUES(?1, ?2, ?3, ?4, ?5)",
        params![revision, ordinal, parent_id, child_id, effect],
    )?;
    Ok(())
}

fn insert_evidence_effect(
    transaction: &Transaction<'_>,
    revision: i64,
    ordinal: i64,
    item: &SnapshotEvidence,
    effect: &str,
) -> Result<(), AppError> {
    transaction.execute(
        "INSERT INTO evidence_link_effects(
             revision, ordinal, concept_id, work_id, start_byte, end_byte, effect
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            revision,
            ordinal,
            item.concept_id,
            item.work_id,
            i64::try_from(item.start_byte).map_err(|_| numeric_overflow())?,
            i64::try_from(item.end_byte).map_err(|_| numeric_overflow())?,
            effect,
        ],
    )?;
    Ok(())
}

fn invalid_effect_row(revision: i64, kind: &str) -> AppError {
    AppError::database(
        "invalid_commit_effect",
        format!("revision {revision} contains an invalid {kind} effect"),
    )
}

fn revision_not_found(revision: i64) -> AppError {
    AppError::not_found(
        "revision_not_found",
        format!("corpus revision {revision} was not found"),
    )
}

fn numeric_overflow() -> AppError {
    AppError::database(
        "numeric_overflow",
        "a corpus count or coordinate is too large",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{SnapshotConcept, SnapshotEdge, SnapshotEvidence};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn typed_effects_replay_every_revision_without_snapshots() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let mut connection = crate::db::init(&path)?;
        connection.execute(
            "INSERT INTO works(label, normalized_label, text, sha256, created_at)
             VALUES('Source', 'source', 'quoted', ?1, 'now')",
            ["0".repeat(64)],
        )?;
        connection.execute("INSERT INTO concept_identities(id) VALUES(1), (2)", [])?;

        let one = CorpusState {
            concepts: vec![SnapshotConcept {
                id: 1,
                label: "Root".into(),
            }],
            edges: vec![],
            evidence: vec![SnapshotEvidence {
                concept_id: 1,
                work_id: 1,
                start_byte: 0,
                end_byte: 6,
            }],
        };
        append_test_commit(&mut connection, 1, &one)?;
        let two = CorpusState {
            concepts: vec![
                SnapshotConcept {
                    id: 1,
                    label: "Broad".into(),
                },
                SnapshotConcept {
                    id: 2,
                    label: "Leaf".into(),
                },
            ],
            edges: vec![SnapshotEdge {
                parent_id: 1,
                child_id: 2,
            }],
            evidence: vec![SnapshotEvidence {
                concept_id: 2,
                work_id: 1,
                start_byte: 0,
                end_byte: 6,
            }],
        };
        append_test_commit(&mut connection, 2, &two)?;

        assert_eq!(
            load_revision_snapshot(&connection, 0)?,
            Some(CorpusState::empty())
        );
        assert_eq!(load_revision_snapshot(&connection, 1)?, Some(one));
        assert_eq!(load_revision_snapshot(&connection, 2)?, Some(two));
        assert_eq!(
            connection.query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name LIKE 'revision_%'",
                [],
                |row| row.get::<_, i64>(0)
            )?,
            0
        );
        Ok(())
    }

    #[test]
    fn graph_projection_is_connection_local_and_derived() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("annals.db");
        let mut connection = crate::db::init(&path)?;
        connection.execute("INSERT INTO concept_identities(id) VALUES(1)", [])?;
        let state = CorpusState {
            concepts: vec![SnapshotConcept {
                id: 1,
                label: "Mixed Case".into(),
            }],
            edges: vec![],
            evidence: vec![],
        };
        append_test_commit(&mut connection, 1, &state)?;
        prepare_graph_revision(&connection, 1)?;
        let row = connection.query_row(
            "SELECT label, normalized_label FROM temp.revision_concepts",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?;
        assert_eq!(row, ("Mixed Case".into(), "mixed case".into()));
        assert!(!connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'revision_concepts')",
            [],
            |row| row.get::<_, bool>(0)
        )?);
        Ok(())
    }

    fn append_test_commit(
        connection: &mut Connection,
        revision: i64,
        state: &CorpusState,
    ) -> Result<(), AppError> {
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO commits(revision, kind, actor, created_at)
             VALUES(?1, 'shake', 'test', 'now')",
            [revision],
        )?;
        insert_canonical_revision(&transaction, revision, state)?;
        transaction.commit()?;
        Ok(())
    }
}
