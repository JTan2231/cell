use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, Transaction, params};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::error::AppError;

pub(crate) const INDEXER_VERSION: i64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IndexStats {
    pub concepts: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IndexStatus {
    pub stored_version: Option<i64>,
    pub concept_count: i64,
    pub indexed_count: i64,
}

impl IndexStatus {
    #[must_use]
    pub fn is_current(self) -> bool {
        self.stored_version == Some(INDEXER_VERSION) && self.concept_count == self.indexed_count
    }
}

#[must_use]
pub(crate) fn normalize(text: &str) -> String {
    let mut normalized = String::new();
    let mut pending_space = false;
    for character in text.nfkc().flat_map(char::to_lowercase) {
        if character.is_whitespace() {
            pending_space = !normalized.is_empty();
        } else {
            if pending_space {
                normalized.push(' ');
                pending_space = false;
            }
            normalized.push(character);
        }
    }
    normalized
}

pub(crate) fn status(connection: &Connection) -> Result<IndexStatus, AppError> {
    let stored_version = connection
        .query_row(
            "SELECT value FROM index_metadata WHERE key = 'indexer_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|value| value.parse::<i64>().ok());
    let concept_count = count(connection, "concepts")?;
    let indexed_count = count(connection, "concept_search")?;
    Ok(IndexStatus {
        stored_version,
        concept_count,
        indexed_count,
    })
}

pub(crate) fn rebuild_all(transaction: &Transaction<'_>) -> Result<IndexStats, AppError> {
    let nodes = load_nodes(transaction)?;
    let edges = load_edges(transaction)?;
    let ancestors = ancestor_ids(&nodes, &edges)?;
    transaction.execute("DELETE FROM concept_search", [])?;

    for (id, node) in &nodes {
        let ancestor_ids = ancestors.get(id).ok_or_else(|| {
            AppError::database(
                "index_ancestors_missing",
                format!("concept {id} has no computed ancestor set"),
            )
        })?;
        let (ancestor_labels, normalized_ancestors) = ancestor_context(&nodes, ancestor_ids)?;
        let normalized_label = normalize(&node.label);
        let content_hash = content_hash(*id, &node.label, &ancestor_labels);
        transaction.execute(
            "INSERT INTO concept_search(\
                 concept_id, label, ancestors, normalized_label, normalized_ancestors, \
                 content_hash, indexer_version\
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                node.label,
                ancestor_labels,
                normalized_label,
                normalized_ancestors,
                content_hash,
                INDEXER_VERSION
            ],
        )?;
    }
    transaction.execute(
        "INSERT INTO index_metadata(key, value) VALUES('indexer_version', ?1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [INDEXER_VERSION.to_string()],
    )?;
    transaction.execute("INSERT INTO concept_fts(concept_fts) VALUES('rebuild')", [])?;
    Ok(IndexStats {
        concepts: nodes.len(),
    })
}

#[derive(Debug)]
struct NodeRow {
    label: String,
}

fn load_nodes(connection: &Connection) -> Result<BTreeMap<i64, NodeRow>, AppError> {
    let mut statement = connection.prepare("SELECT id, label FROM concepts ORDER BY id")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, NodeRow { label: row.get(1)? }))
    })?;
    Ok(rows.collect::<Result<BTreeMap<_, _>, _>>()?)
}

fn load_edges(connection: &Connection) -> Result<Vec<(i64, i64)>, AppError> {
    let mut statement = connection
        .prepare("SELECT parent_id, child_id FROM concept_edges ORDER BY parent_id, child_id")?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn ancestor_ids(
    nodes: &BTreeMap<i64, NodeRow>,
    edges: &[(i64, i64)],
) -> Result<BTreeMap<i64, BTreeSet<i64>>, AppError> {
    let mut ancestors = nodes
        .keys()
        .map(|id| (*id, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut indegree = nodes
        .keys()
        .map(|id| (*id, 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut children = BTreeMap::<i64, BTreeSet<i64>>::new();

    for &(parent, child) in edges {
        if parent == child {
            return Err(AppError::database(
                "concept_self_edge",
                format!("concept {parent} is its own parent"),
            ));
        }
        if !nodes.contains_key(&parent) || !nodes.contains_key(&child) {
            return Err(AppError::database(
                "concept_edge_endpoint_missing",
                format!("concept edge {parent} -> {child} has a missing endpoint"),
            ));
        }
        if !children.entry(parent).or_default().insert(child) {
            return Err(AppError::database(
                "duplicate_concept_edge",
                format!("concept edge {parent} -> {child} is duplicated"),
            ));
        }
        let degree = indegree.get_mut(&child).ok_or_else(|| {
            AppError::database(
                "concept_edge_endpoint_missing",
                format!("concept edge {parent} -> {child} has a missing child"),
            )
        })?;
        *degree = degree.checked_add(1).ok_or_else(|| {
            AppError::database(
                "concept_indegree_overflow",
                "a concept has too many parents",
            )
        })?;
    }

    let mut ready = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
        .collect::<BTreeSet<_>>();
    let mut visited = 0_usize;
    while let Some(parent) = ready.pop_first() {
        visited += 1;
        let mut inherited = ancestors.get(&parent).cloned().ok_or_else(|| {
            AppError::database(
                "index_ancestors_missing",
                format!("concept {parent} has no computed ancestor set"),
            )
        })?;
        inherited.insert(parent);
        for child in children.get(&parent).into_iter().flatten() {
            ancestors
                .get_mut(child)
                .ok_or_else(|| {
                    AppError::database(
                        "concept_edge_endpoint_missing",
                        format!("concept edge {parent} -> {child} has a missing child"),
                    )
                })?
                .extend(inherited.iter().copied());
            let degree = indegree.get_mut(child).ok_or_else(|| {
                AppError::database(
                    "concept_edge_endpoint_missing",
                    format!("concept edge {parent} -> {child} has a missing child"),
                )
            })?;
            *degree = degree.checked_sub(1).ok_or_else(|| {
                AppError::database(
                    "invalid_concept_indegree",
                    "a concept edge was processed more than once",
                )
            })?;
            if *degree == 0 {
                ready.insert(*child);
            }
        }
    }
    if visited != nodes.len() {
        return Err(AppError::database(
            "concept_cycle",
            "the concept graph contains a cycle",
        ));
    }
    Ok(ancestors)
}

fn ancestor_context(
    nodes: &BTreeMap<i64, NodeRow>,
    ancestors: &BTreeSet<i64>,
) -> Result<(String, String), AppError> {
    let mut labels = ancestors
        .iter()
        .map(|id| {
            let node = nodes.get(id).ok_or_else(|| {
                AppError::database(
                    "index_ancestor_missing",
                    format!("computed ancestor {id} is missing"),
                )
            })?;
            Ok((normalize(&node.label), node.label.clone(), *id))
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    labels.sort();

    let mut exact = Vec::new();
    let mut normalized = Vec::new();
    for (normalized_label, label, _) in labels {
        if exact.last() != Some(&label) {
            exact.push(label);
        }
        if normalized.last() != Some(&normalized_label) {
            normalized.push(normalized_label);
        }
    }
    Ok((exact.join("\n"), normalized.join(" ")))
}

fn content_hash(id: i64, label: &str, ancestors: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(id.to_le_bytes());
    digest.update(label.as_bytes());
    digest.update([0]);
    digest.update(ancestors.as_bytes());
    digest.update(INDEXER_VERSION.to_le_bytes());
    format!("{:x}", digest.finalize())
}

fn count(connection: &Connection, table: &str) -> Result<i64, AppError> {
    Ok(
        connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })?,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use rusqlite::Connection;

    use super::{NodeRow, ancestor_context, ancestor_ids, normalize, rebuild_all};

    fn nodes(entries: &[(i64, &str)]) -> BTreeMap<i64, NodeRow> {
        entries
            .iter()
            .map(|(id, label)| {
                (
                    *id,
                    NodeRow {
                        label: (*label).to_owned(),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn normalization_is_language_stable() {
        assert_eq!(normalize("  Predicate\tLOCKING  "), "predicate locking");
        assert_eq!(normalize("Ａ"), "a");
    }

    #[test]
    fn diamond_ancestry_is_deduplicated() {
        let graph = nodes(&[(1, "A"), (2, "B"), (3, "Shared"), (4, "Leaf")]);
        let ancestors = ancestor_ids(&graph, &[(1, 3), (2, 3), (3, 4)])
            .unwrap_or_else(|error| panic!("valid graph was rejected: {error}"));
        assert_eq!(ancestors[&4], BTreeSet::from([1, 2, 3]));
        let context = ancestor_context(&graph, &ancestors[&4])
            .unwrap_or_else(|error| panic!("ancestor context failed: {error}"));
        assert_eq!(
            context,
            ("A\nB\nShared".to_owned(), "a b shared".to_owned())
        );
    }

    #[test]
    fn duplicate_ancestor_labels_add_one_search_term() {
        let graph = nodes(&[(1, "Shared"), (2, "SHARED"), (3, "Leaf")]);
        let ancestors = ancestor_ids(&graph, &[(1, 3), (2, 3)])
            .unwrap_or_else(|error| panic!("valid graph was rejected: {error}"));
        let (exact, normalized) = ancestor_context(&graph, &ancestors[&3])
            .unwrap_or_else(|error| panic!("ancestor context failed: {error}"));
        assert_eq!(exact, "SHARED\nShared");
        assert_eq!(normalized, "shared");
    }

    #[test]
    fn invalid_graphs_cannot_be_indexed() {
        let graph = nodes(&[(1, "A"), (2, "B")]);
        let Err(cycle_error) = ancestor_ids(&graph, &[(1, 2), (2, 1)]) else {
            panic!("cycle was accepted");
        };
        assert_eq!(cycle_error.code(), "concept_cycle");
        let Err(endpoint_error) = ancestor_ids(&graph, &[(1, 3)]) else {
            panic!("missing endpoint was accepted");
        };
        assert_eq!(endpoint_error.code(), "concept_edge_endpoint_missing");
    }

    #[test]
    fn rebuild_writes_one_ancestor_context_row_per_concept()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch(include_str!("../schema.sql"))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let transaction = connection.transaction()?;
        transaction.execute("INSERT INTO concepts(id, label) VALUES(1, 'A')", [])?;
        transaction.execute("INSERT INTO concepts(id, label) VALUES(2, 'B')", [])?;
        transaction.execute("INSERT INTO concepts(id, label) VALUES(3, 'Leaf')", [])?;
        transaction.execute(
            "INSERT INTO concept_edges(parent_id, child_id) VALUES(1, 3), (2, 3)",
            [],
        )?;

        let stats = rebuild_all(&transaction)?;
        assert_eq!(stats.concepts, 3);
        let row = transaction.query_row(
            "SELECT ancestors, normalized_ancestors, length(content_hash), indexer_version \
             FROM concept_search WHERE concept_id = 3",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )?;
        assert_eq!(row, ("A\nB".to_owned(), "a b".to_owned(), 64, 2));
        Ok(())
    }
}
