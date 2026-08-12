use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::error::AppError;
use crate::tree;

/// Version of the deterministic node-text index representation.
pub const INDEXER_VERSION: i64 = 2;
pub const INDEXER_VERSION_KEY: &str = "indexer_version";
pub const BREADCRUMB_SEPARATOR: &str = " / ";

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// One canonical node transformed into its single derived search row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedUnit {
    pub node_id: i64,
    pub text: String,
    pub normalized_text: String,
    pub breadcrumb: String,
    pub normalized_path: String,
    pub content_hash: String,
    pub indexer_version: i64,
}

/// Summary returned by a complete derived-index rebuild.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RebuildStats {
    pub nodes: usize,
    pub units: usize,
}

/// Cheap derived-index freshness state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Current,
    MissingVersion,
    InvalidVersion(String),
    VersionMismatch { stored: i64, expected: i64 },
    IncompatibleUnits { unit_count: i64 },
    UnitCountMismatch { nodes: i64, units: i64 },
}

impl Status {
    #[must_use]
    pub const fn is_current(&self) -> bool {
        matches!(self, Self::Current)
    }
}

/// Normalize conceptual strings and paths for equality and exact lookup.
#[must_use]
pub fn normalize_key(input: &str) -> String {
    let mut normalized = String::new();
    let mut pending_space = false;
    for character in input.nfkc().flat_map(char::to_lowercase) {
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

/// Derive the one search row belonging to a canonical node.
#[must_use]
pub fn derive_unit(node_id: i64, text: &str, breadcrumb: &str) -> DerivedUnit {
    let normalized_text = normalize_key(text);
    let normalized_path = normalize_key(breadcrumb);
    let content_hash = unit_hash(
        node_id,
        text,
        &normalized_text,
        breadcrumb,
        &normalized_path,
    );
    DerivedUnit {
        node_id,
        text: text.to_owned(),
        normalized_text,
        breadcrumb: breadcrumb.to_owned(),
        normalized_path,
        content_hash,
        indexer_version: INDEXER_VERSION,
    }
}

/// Recreate every derived search row from canonical nodes.
pub fn rebuild_all(transaction: &Transaction<'_>) -> Result<RebuildStats, AppError> {
    transaction
        .execute("DELETE FROM search_units", [])
        .map_err(|error| database_error("index_clear_failed", &error))?;

    let nodes = {
        let mut statement = transaction
            .prepare("SELECT id, text FROM nodes ORDER BY id")
            .map_err(|error| database_error("index_node_read_failed", &error))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| database_error("index_node_read_failed", &error))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| database_error("index_node_read_failed", &error))?
    };

    for (node_id, text) in &nodes {
        let breadcrumb = tree::node_path(transaction, *node_id)?;
        let unit = derive_unit(*node_id, text, &breadcrumb);
        insert_unit(transaction, &unit)?;
    }

    transaction
        .execute("INSERT INTO search_fts(search_fts) VALUES ('rebuild')", [])
        .map_err(|error| database_error("fts_rebuild_failed", &error))?;
    set_current_version(transaction)?;
    Ok(RebuildStats {
        nodes: nodes.len(),
        units: nodes.len(),
    })
}

fn insert_unit(transaction: &Transaction<'_>, unit: &DerivedUnit) -> Result<(), AppError> {
    transaction
        .execute(
            "INSERT INTO search_units(\
                 node_id, text, normalized_text, breadcrumb, normalized_path, \
                 content_hash, indexer_version\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                unit.node_id,
                unit.text,
                unit.normalized_text,
                unit.breadcrumb,
                unit.normalized_path,
                unit.content_hash,
                unit.indexer_version,
            ],
        )
        .map_err(|error| database_error("index_unit_write_failed", &error))?;
    Ok(())
}

/// Inspect version and row-count indicators without mutating the index.
pub fn status(connection: &Connection) -> Result<Status, AppError> {
    let stored = connection
        .query_row(
            "SELECT value FROM index_metadata WHERE key = ?1",
            [INDEXER_VERSION_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| database_error("index_metadata_read_failed", &error))?;
    let Some(stored) = stored else {
        return Ok(Status::MissingVersion);
    };
    let Ok(stored_version) = stored.parse::<i64>() else {
        return Ok(Status::InvalidVersion(stored));
    };
    if stored_version != INDEXER_VERSION {
        return Ok(Status::VersionMismatch {
            stored: stored_version,
            expected: INDEXER_VERSION,
        });
    }
    let incompatible_units = connection
        .query_row(
            "SELECT COUNT(*) FROM search_units WHERE indexer_version <> ?1",
            [INDEXER_VERSION],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| database_error("index_status_failed", &error))?;
    if incompatible_units != 0 {
        return Ok(Status::IncompatibleUnits {
            unit_count: incompatible_units,
        });
    }
    let nodes = count(connection, "SELECT COUNT(*) FROM nodes")?;
    let units = count(connection, "SELECT COUNT(*) FROM search_units")?;
    if nodes != units {
        return Ok(Status::UnitCountMismatch { nodes, units });
    }
    Ok(Status::Current)
}

/// Reject search against missing, stale, or incomplete derived data.
pub fn require_current(connection: &Connection) -> Result<(), AppError> {
    let state = status(connection)?;
    if state.is_current() {
        return Ok(());
    }
    Err(AppError::database(
        "reindex_required",
        format!("the search index is not current ({state:?}); run `annals reindex`"),
    ))
}

/// Store the active index representation version.
pub fn set_current_version(transaction: &Transaction<'_>) -> Result<(), AppError> {
    transaction
        .execute(
            "INSERT INTO index_metadata(key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![INDEXER_VERSION_KEY, INDEXER_VERSION.to_string()],
        )
        .map_err(|error| database_error("index_metadata_write_failed", &error))?;
    Ok(())
}

fn count(connection: &Connection, sql: &str) -> Result<i64, AppError> {
    connection
        .query_row(sql, [], |row| row.get(0))
        .map_err(|error| database_error("index_status_failed", &error))
}

fn unit_hash(
    node_id: i64,
    text: &str,
    normalized_text: &str,
    breadcrumb: &str,
    normalized_path: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(node_id.to_be_bytes());
    hash_text(&mut hasher, text);
    hash_text(&mut hasher, normalized_text);
    hash_text(&mut hasher, breadcrumb);
    hash_text(&mut hasher, normalized_path);
    hasher.update(INDEXER_VERSION.to_be_bytes());
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(7 + digest.len() * 2);
    encoded.push_str("sha256:");
    for byte in digest {
        encoded.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn hash_text(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_string().as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
}

fn database_error(code: &'static str, error: &rusqlite::Error) -> AppError {
    AppError::database(code, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{INDEXER_VERSION, derive_unit, normalize_key};

    #[test]
    fn normalization_is_unicode_aware_and_collapses_space() {
        assert_eq!(normalize_key("  Ｒust\n TREE  "), "rust tree");
    }

    #[test]
    fn derived_unit_is_deterministic() {
        let first = derive_unit(7, "Write skew", "Databases / Write skew");
        let second = derive_unit(7, "Write skew", "Databases / Write skew");
        assert_eq!(first, second);
        assert_eq!(first.indexer_version, INDEXER_VERSION);
        assert!(first.content_hash.starts_with("sha256:"));
    }
}
