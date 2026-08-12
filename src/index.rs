use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, Transaction, params};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::error::AppError;

pub(crate) const INDEXER_VERSION: i64 = 1;

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

pub(crate) fn require_current(connection: &Connection) -> Result<(), AppError> {
    if status(connection)?.is_current() {
        Ok(())
    } else {
        Err(AppError::conflict(
            "reindex_required",
            "the concept search index is stale; run `annals reindex`",
        ))
    }
}

pub(crate) fn rebuild_all(transaction: &Transaction<'_>) -> Result<IndexStats, AppError> {
    let nodes = load_nodes(transaction)?;
    let paths = all_paths(&nodes)?;
    transaction.execute("DELETE FROM concept_search", [])?;

    for (id, node) in &nodes {
        let segments = paths.get(id).ok_or_else(|| {
            AppError::database("index_path_missing", format!("concept {id} has no path"))
        })?;
        let path = segments.join(" › ");
        let normalized_label = normalize(&node.label);
        let normalized_path = normalize(&path);
        let content_hash = content_hash(*id, &node.label, &path);
        transaction.execute(
            "INSERT INTO concept_search(\
                 id, concept_id, label, path, normalized_label, normalized_path, content_hash, \
                 indexer_version\
             ) VALUES(?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                node.label,
                path,
                normalized_label,
                normalized_path,
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
    parent: Option<i64>,
    label: String,
}

fn load_nodes(connection: &Connection) -> Result<BTreeMap<i64, NodeRow>, AppError> {
    let mut statement = connection.prepare("SELECT id, parent_id, label FROM concepts")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            NodeRow {
                parent: row.get(1)?,
                label: row.get(2)?,
            },
        ))
    })?;
    Ok(rows.collect::<Result<BTreeMap<_, _>, _>>()?)
}

fn all_paths(nodes: &BTreeMap<i64, NodeRow>) -> Result<BTreeMap<i64, Vec<String>>, AppError> {
    let mut paths = BTreeMap::new();
    for id in nodes.keys().copied() {
        let mut reverse = Vec::new();
        let mut current = Some(id);
        let mut seen = BTreeSet::new();
        while let Some(next) = current {
            if !seen.insert(next) {
                return Err(AppError::database(
                    "concept_cycle",
                    format!("concept {id} belongs to a parent cycle"),
                ));
            }
            let node = nodes.get(&next).ok_or_else(|| {
                AppError::database(
                    "concept_parent_missing",
                    format!("concept {id} references missing parent {next}"),
                )
            })?;
            reverse.push(node.label.clone());
            current = node.parent;
        }
        reverse.reverse();
        paths.insert(id, reverse);
    }
    Ok(paths)
}

fn content_hash(id: i64, label: &str, path: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(id.to_le_bytes());
    digest.update(label.as_bytes());
    digest.update([0]);
    digest.update(path.as_bytes());
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
    use super::normalize;

    #[test]
    fn normalization_is_language_stable() {
        assert_eq!(normalize("  Predicate\tLOCKING  "), "predicate locking");
        assert_eq!(normalize("Ａ"), "a");
    }
}
