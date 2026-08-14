use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::corpus::{Snapshot, SnapshotConcept, SnapshotEdge, SnapshotEvidence};
use crate::error::AppError;
use crate::index;

/// Row counts recorded with one immutable relational corpus snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RevisionStats {
    pub concepts: i64,
    pub edges: i64,
    pub evidence: i64,
}

impl RevisionStats {
    const EMPTY: Self = Self {
        concepts: 0,
        edges: 0,
        evidence: 0,
    };
}

/// Copy the complete canonical corpus into immutable rows for its current revision.
///
/// The caller must invoke this after inserting the matching commit, in the same
/// transaction that materialized canonical HEAD. A second insertion is rejected.
pub(crate) fn insert_canonical_revision(
    transaction: &Transaction<'_>,
    revision: i64,
    expected: &Snapshot,
) -> Result<RevisionStats, AppError> {
    if revision <= 0 {
        return Err(AppError::invalid(
            "invalid_revision",
            "only positive committed revisions can be stored",
        ));
    }

    let head = transaction.query_row(
        "SELECT revision FROM library_state WHERE singleton = 1",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    if head != revision {
        return Err(AppError::conflict(
            "revision_snapshot_not_head",
            format!("cannot store revision {revision} while canonical HEAD is revision {head}"),
        ));
    }
    if !commit_exists(transaction, revision)? {
        return Err(AppError::database(
            "revision_commit_missing",
            format!("cannot store revision {revision} because its commit is missing"),
        ));
    }
    if stored_revision_stats(transaction, revision)?.is_some() {
        return Err(AppError::conflict(
            "revision_snapshot_exists",
            format!("revision {revision} already has a stored relational snapshot"),
        ));
    }

    let stats = RevisionStats {
        concepts: table_count(transaction, "concepts")?,
        edges: table_count(transaction, "concept_edges")?,
        evidence: table_count(transaction, "evidence")?,
    };
    require_loaded_count(
        "expected concept",
        revision,
        expected.concepts.len(),
        stats.concepts,
    )?;
    require_loaded_count("expected edge", revision, expected.edges.len(), stats.edges)?;
    require_loaded_count(
        "expected evidence",
        revision,
        expected.evidence.len(),
        stats.evidence,
    )?;
    transaction.execute(
        "INSERT INTO revision_snapshots(\
             revision, concept_count, edge_count, evidence_count\
         ) VALUES(?1, ?2, ?3, ?4)",
        params![revision, stats.concepts, stats.edges, stats.evidence],
    )?;

    let concepts = transaction.execute(
        "INSERT INTO revision_concepts(\
             revision, concept_id, label, normalized_label, \
             parent_count, child_count, evidence_count\
         ) \
         SELECT ?1, concepts.id, concepts.label, concept_search.normalized_label, \
                (SELECT COUNT(*) FROM concept_edges AS parents \
                 WHERE parents.child_id = concepts.id), \
                (SELECT COUNT(*) FROM concept_edges AS children \
                 WHERE children.parent_id = concepts.id), \
                (SELECT COUNT(*) FROM evidence WHERE evidence.concept_id = concepts.id) \
         FROM concepts JOIN concept_search ON concept_search.concept_id = concepts.id \
         ORDER BY concepts.id",
        [revision],
    )?;
    let edges = transaction.execute(
        "INSERT INTO revision_edges(revision, parent_id, child_id) \
         SELECT ?1, parent_id, child_id FROM concept_edges ORDER BY parent_id, child_id",
        [revision],
    )?;
    let evidence = transaction.execute(
        "INSERT INTO revision_evidence(\
             revision, concept_id, work_id, start_byte, end_byte\
         ) SELECT ?1, concept_id, work_id, start_byte, end_byte \
           FROM evidence ORDER BY concept_id, work_id, start_byte, end_byte",
        [revision],
    )?;

    require_insert_count("concept", revision, concepts, stats.concepts)?;
    require_insert_count("edge", revision, edges, stats.edges)?;
    require_insert_count("evidence", revision, evidence, stats.evidence)?;
    require_revision_matches(transaction, revision, expected)?;
    Ok(stats)
}

/// Return whether a complete relational snapshot is available for a revision.
///
/// Revision zero is the implicit empty corpus and exists whenever library state
/// exists; positive revisions require a `revision_snapshots` marker row.
pub(crate) fn revision_exists(connection: &Connection, revision: i64) -> Result<bool, AppError> {
    Ok(revision_stats(connection, revision)?.is_some())
}

/// Return the stored row counts for a revision, if that revision is available.
pub(crate) fn revision_stats(
    connection: &Connection,
    revision: i64,
) -> Result<Option<RevisionStats>, AppError> {
    if revision < 0 {
        return Ok(None);
    }
    if revision == 0 {
        let initialized = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM library_state WHERE singleton = 1)",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        return Ok(initialized.then_some(RevisionStats::EMPTY));
    }
    stored_revision_stats(connection, revision)
}

/// Load one complete revision for validation or another explicitly whole-graph operation.
pub(crate) fn load_revision_snapshot(
    connection: &Connection,
    revision: i64,
) -> Result<Option<Snapshot>, AppError> {
    let Some(stats) = revision_stats(connection, revision)? else {
        return Ok(None);
    };
    if revision == 0 {
        return Ok(Some(Snapshot::empty()));
    }

    let mut concepts_statement = connection.prepare(
        "SELECT concept_id, label, normalized_label FROM revision_concepts \
         WHERE revision = ?1 ORDER BY concept_id",
    )?;
    let concept_rows = concepts_statement.query_map([revision], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut concepts = Vec::new();
    for row in concept_rows {
        let (id, label, normalized_label) = row?;
        if index::normalize(&label) != normalized_label {
            return Err(AppError::database(
                "revision_normalization_mismatch",
                format!("revision {revision} concept c{id} has a stale normalized label"),
            ));
        }
        concepts.push(SnapshotConcept { id, label });
    }

    let mut edges_statement = connection.prepare(
        "SELECT parent_id, child_id FROM revision_edges \
         WHERE revision = ?1 ORDER BY parent_id, child_id",
    )?;
    let edges = edges_statement
        .query_map([revision], |row| {
            Ok(SnapshotEdge {
                parent_id: row.get(0)?,
                child_id: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut evidence_statement = connection.prepare(
        "SELECT concept_id, work_id, start_byte, end_byte FROM revision_evidence \
         WHERE revision = ?1 ORDER BY concept_id, work_id, start_byte, end_byte",
    )?;
    let evidence = evidence_statement
        .query_map([revision], |row| {
            Ok(SnapshotEvidence {
                concept_id: row.get(0)?,
                work_id: row.get(1)?,
                start_byte: usize_from_i64(row.get(2)?, "evidence start byte")?,
                end_byte: usize_from_i64(row.get(3)?, "evidence end byte")?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    require_loaded_count("concept", revision, concepts.len(), stats.concepts)?;
    require_loaded_count("edge", revision, edges.len(), stats.edges)?;
    require_loaded_count("evidence", revision, evidence.len(), stats.evidence)?;
    let counts_match = connection.query_row(
        "SELECT NOT EXISTS(\
             SELECT 1 FROM revision_concepts AS concept \
             WHERE concept.revision = ?1 AND (\
                 concept.parent_count != (\
                     SELECT COUNT(*) FROM revision_edges AS edge \
                     WHERE edge.revision = concept.revision \
                       AND edge.child_id = concept.concept_id\
                 ) OR concept.child_count != (\
                     SELECT COUNT(*) FROM revision_edges AS edge \
                     WHERE edge.revision = concept.revision \
                       AND edge.parent_id = concept.concept_id\
                 ) OR concept.evidence_count != (\
                     SELECT COUNT(*) FROM revision_evidence AS evidence \
                     WHERE evidence.revision = concept.revision \
                       AND evidence.concept_id = concept.concept_id\
                 )\
             )\
         )",
        [revision],
        |row| row.get::<_, bool>(0),
    )?;
    if !counts_match {
        return Err(AppError::database(
            "revision_degree_mismatch",
            format!("revision {revision} has stale stored graph counts"),
        ));
    }
    Ok(Some(Snapshot {
        concepts,
        edges,
        evidence,
    }))
}

fn stored_revision_stats(
    connection: &Connection,
    revision: i64,
) -> Result<Option<RevisionStats>, AppError> {
    Ok(connection
        .query_row(
            "SELECT concept_count, edge_count, evidence_count \
             FROM revision_snapshots WHERE revision = ?1",
            [revision],
            |row| {
                Ok(RevisionStats {
                    concepts: row.get(0)?,
                    edges: row.get(1)?,
                    evidence: row.get(2)?,
                })
            },
        )
        .optional()?)
}

fn commit_exists(connection: &Connection, revision: i64) -> Result<bool, AppError> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM commits WHERE revision = ?1)",
        [revision],
        |row| row.get(0),
    )?)
}

fn table_count(connection: &Connection, table: &str) -> Result<i64, AppError> {
    Ok(
        connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })?,
    )
}

fn require_insert_count(
    kind: &str,
    revision: i64,
    inserted: usize,
    expected: i64,
) -> Result<(), AppError> {
    if i64::try_from(inserted) == Ok(expected) {
        Ok(())
    } else {
        Err(AppError::database(
            "revision_snapshot_incomplete",
            format!("revision {revision} stored {inserted} {kind} rows, expected {expected}"),
        ))
    }
}

fn require_loaded_count(
    kind: &str,
    revision: i64,
    loaded: usize,
    expected: i64,
) -> Result<(), AppError> {
    if i64::try_from(loaded) == Ok(expected) {
        Ok(())
    } else {
        Err(AppError::database(
            "revision_snapshot_incomplete",
            format!("revision {revision} loaded {loaded} {kind} rows, expected {expected}"),
        ))
    }
}

fn require_revision_matches(
    connection: &Connection,
    revision: i64,
    expected: &Snapshot,
) -> Result<(), AppError> {
    let mut concepts = connection.prepare(
        "SELECT concept_id, label, normalized_label FROM revision_concepts \
         WHERE revision = ?1 ORDER BY concept_id",
    )?;
    for (row, expected) in concepts
        .query_map([revision], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .zip(&expected.concepts)
    {
        let (id, label, normalized_label) = row?;
        if id != expected.id
            || label != expected.label
            || normalized_label != index::normalize(&expected.label)
        {
            return Err(revision_mismatch(revision));
        }
    }

    let mut edges = connection.prepare(
        "SELECT parent_id, child_id FROM revision_edges \
         WHERE revision = ?1 ORDER BY parent_id, child_id",
    )?;
    for (row, expected) in edges
        .query_map([revision], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?
        .zip(&expected.edges)
    {
        let (parent_id, child_id) = row?;
        if parent_id != expected.parent_id || child_id != expected.child_id {
            return Err(revision_mismatch(revision));
        }
    }

    let mut evidence = connection.prepare(
        "SELECT concept_id, work_id, start_byte, end_byte FROM revision_evidence \
         WHERE revision = ?1 ORDER BY concept_id, work_id, start_byte, end_byte",
    )?;
    for (row, expected) in evidence
        .query_map([revision], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                usize_from_i64(row.get(2)?, "evidence start byte")?,
                usize_from_i64(row.get(3)?, "evidence end byte")?,
            ))
        })?
        .zip(&expected.evidence)
    {
        let (concept_id, work_id, start_byte, end_byte) = row?;
        if concept_id != expected.concept_id
            || work_id != expected.work_id
            || start_byte != expected.start_byte
            || end_byte != expected.end_byte
        {
            return Err(revision_mismatch(revision));
        }
    }
    Ok(())
}

fn revision_mismatch(revision: i64) -> AppError {
    AppError::database(
        "revision_snapshot_mismatch",
        format!("revision {revision} does not match the materialized canonical graph"),
    )
}

fn usize_from_i64(value: i64, description: &str) -> rusqlite::Result<usize> {
    usize::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::other(format!(
                "invalid {description}: {error}"
            ))),
        )
    })
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::{
        RevisionStats, insert_canonical_revision, load_revision_snapshot, revision_exists,
        revision_stats,
    };
    use crate::corpus::{Snapshot, SnapshotConcept, SnapshotEdge, SnapshotEvidence};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn stores_one_full_revision_without_following_head_mutations() -> TestResult {
        let mut connection = test_connection()?;
        seed_revision_one(&connection)?;
        let expected = revision_one_snapshot();

        let transaction = connection.transaction()?;
        let stats = insert_canonical_revision(&transaction, 1, &expected)?;
        transaction.commit()?;
        assert_eq!(
            stats,
            RevisionStats {
                concepts: 2,
                edges: 1,
                evidence: 1
            }
        );
        assert!(revision_exists(&connection, 0)?);
        assert!(revision_exists(&connection, 1)?);
        assert!(!revision_exists(&connection, 2)?);

        connection.execute("UPDATE concepts SET label = 'Changed' WHERE id = 2", [])?;
        let stored = connection.query_row(
            "SELECT label FROM revision_concepts WHERE revision = 1 AND concept_id = 2",
            [],
            |row| row.get::<_, String>(0),
        )?;
        assert_eq!(stored, "Leaf");
        assert!(
            connection
                .execute(
                    "UPDATE revision_concepts SET label = 'Tampered' \
                     WHERE revision = 1 AND concept_id = 2",
                    [],
                )
                .is_err(),
            "a stored revision concept was mutable"
        );
        assert_eq!(revision_stats(&connection, 1)?, Some(stats));
        let loaded = load_revision_snapshot(&connection, 1)?.ok_or("missing stored revision")?;
        assert_eq!(loaded.concepts.len(), 2);
        assert_eq!(loaded.edges.len(), 1);
        assert_eq!(loaded.evidence.len(), 1);
        Ok(())
    }

    #[test]
    fn rejects_duplicate_and_non_head_insertions() -> TestResult {
        let mut connection = test_connection()?;
        seed_revision_one(&connection)?;
        let expected = revision_one_snapshot();
        let transaction = connection.transaction()?;
        insert_canonical_revision(&transaction, 1, &expected)?;
        transaction.commit()?;

        let transaction = connection.transaction()?;
        let Err(duplicate) = insert_canonical_revision(&transaction, 1, &expected) else {
            return Err("a revision snapshot must be immutable".into());
        };
        assert_eq!(duplicate.code(), "revision_snapshot_exists");
        transaction.rollback()?;

        let transaction = connection.transaction()?;
        let Err(not_head) = insert_canonical_revision(&transaction, 2, &expected) else {
            return Err("a non-HEAD revision cannot copy canonical tables".into());
        };
        assert_eq!(not_head.code(), "revision_snapshot_not_head");
        transaction.rollback()?;
        Ok(())
    }

    #[test]
    fn rejects_a_projection_that_differs_from_the_committed_snapshot() -> TestResult {
        let mut connection = test_connection()?;
        seed_revision_one(&connection)?;
        let mut expected = revision_one_snapshot();
        expected.concepts[1].label = "Different".to_owned();

        let transaction = connection.transaction()?;
        let Err(error) = insert_canonical_revision(&transaction, 1, &expected) else {
            return Err("a mismatched relational revision was accepted".into());
        };
        assert_eq!(error.code(), "revision_snapshot_mismatch");
        transaction.rollback()?;
        assert!(!revision_exists(&connection, 1)?);
        Ok(())
    }

    #[test]
    fn detects_stale_stored_graph_counts() -> TestResult {
        let mut connection = test_connection()?;
        seed_revision_one(&connection)?;
        let expected = revision_one_snapshot();
        let transaction = connection.transaction()?;
        insert_canonical_revision(&transaction, 1, &expected)?;
        transaction.commit()?;

        connection.execute("DROP TRIGGER revision_concepts_immutable_update", [])?;
        connection.execute(
            "UPDATE revision_concepts SET parent_count = parent_count + 1 \
             WHERE revision = 1 AND concept_id = 2",
            [],
        )?;
        let Err(error) = load_revision_snapshot(&connection, 1) else {
            return Err("stale revision graph counts were accepted".into());
        };
        assert_eq!(error.code(), "revision_degree_mismatch");
        Ok(())
    }

    fn test_connection() -> TestResult<Connection> {
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.execute_batch(include_str!("../schema.sql"))?;
        Ok(connection)
    }

    fn seed_revision_one(connection: &Connection) -> TestResult {
        connection.execute(
            "INSERT INTO works(label, normalized_label, text, sha256, created_at) \
             VALUES('Work', 'work', 'fact', ?1, '2026-01-01T00:00:00Z')",
            ["0".repeat(64)],
        )?;
        connection.execute(
            "INSERT INTO concepts(id, label) VALUES(1, 'Root'), (2, 'Leaf')",
            [],
        )?;
        connection.execute(
            "INSERT INTO concept_edges(parent_id, child_id) VALUES(1, 2)",
            [],
        )?;
        connection.execute(
            "INSERT INTO concept_search(\
                 concept_id, label, ancestors, normalized_label, normalized_ancestors, \
                 content_hash, indexer_version\
             ) VALUES(1, 'Root', '', 'root', '', 'root', 1), \
                     (2, 'Leaf', 'Root', 'leaf', 'root', 'leaf', 1)",
            [],
        )?;
        connection.execute(
            "INSERT INTO evidence(concept_id, work_id, start_byte, end_byte) \
             VALUES(2, 1, 0, 4)",
            [],
        )?;
        connection.execute(
            "INSERT INTO commits(\
                 revision, parent_revision, base_revision, kind, summary, submitted_request, \
                 resolved_operations, after_snapshot, metadata, actor, created_at\
             ) VALUES(1, 0, 0, 'revert', 'Fixture', '{}', '[]', \
                      '{\"concepts\":[],\"edges\":[],\"evidence\":[]}', '{}', 'test', \
                      '2026-01-01T00:00:00Z')",
            [],
        )?;
        connection.execute(
            "UPDATE library_state SET revision = 1 WHERE singleton = 1",
            [],
        )?;
        Ok(())
    }

    fn revision_one_snapshot() -> Snapshot {
        Snapshot {
            concepts: vec![
                SnapshotConcept {
                    id: 1,
                    label: "Root".to_owned(),
                },
                SnapshotConcept {
                    id: 2,
                    label: "Leaf".to_owned(),
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
                end_byte: 4,
            }],
        }
    }
}
