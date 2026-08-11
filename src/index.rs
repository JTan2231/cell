use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::error::AppError;

/// The version of the deterministic conversion from nodes to search units.
pub const INDEXER_VERSION: i64 = 1;

/// The metadata key holding [`INDEXER_VERSION`].
pub const INDEXER_VERSION_KEY: &str = "indexer_version";

/// Titles in a breadcrumb are separated by this display delimiter.
pub const BREADCRUMB_SEPARATOR: &str = " / ";

const TARGET_PASSAGE_WORDS: usize = 1_200;
const MIN_NATURAL_PASSAGE_WORDS: usize = 1_000;
const MAX_PASSAGE_WORDS: usize = 1_500;
const PASSAGE_OVERLAP_WORDS: usize = 100;
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// A derived row ready to insert into `search_units`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DerivedUnit {
    pub node_id: i64,
    pub unit_no: i64,
    pub unit_kind: UnitKind,
    pub title: String,
    pub normalized_title: String,
    pub breadcrumb: String,
    pub normalized_path: String,
    pub text: String,
    pub start_byte: Option<i64>,
    pub end_byte: Option<i64>,
    pub content_hash: String,
    pub indexer_version: i64,
}

/// The kind of a derived search unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnitKind {
    Node,
    Passage,
}

impl UnitKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Passage => "passage",
        }
    }
}

/// A UTF-8-safe passage range in a canonical node body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Passage {
    pub start_byte: usize,
    pub end_byte: usize,
}

impl Passage {
    /// Borrow this passage from its canonical body.
    #[must_use]
    pub fn text<'body>(&self, body: &'body str) -> Option<&'body str> {
        body.get(self.start_byte..self.end_byte)
    }
}

/// Counts produced by an indexing operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RebuildStats {
    pub nodes: usize,
    pub units: usize,
}

/// Whether the derived index can be searched by this executable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Status {
    Current,
    MissingVersion,
    InvalidVersion(String),
    VersionMismatch { stored: i64, expected: i64 },
    MissingUnits { node_count: i64 },
    IncompatibleUnits { unit_count: i64 },
}

impl Status {
    #[must_use]
    pub const fn is_current(&self) -> bool {
        matches!(self, Self::Current)
    }
}

#[derive(Clone, Copy, Debug)]
struct WordSpan {
    start: usize,
    end: usize,
}

#[derive(Debug)]
struct NodeText {
    id: i64,
    title: String,
    body: String,
    breadcrumb: String,
}

/// Build the shared key used for normalized title and path lookups.
///
/// Normalization trims Unicode whitespace, applies NFKC followed by Unicode
/// lowercase expansion, and collapses internal whitespace to one ASCII space.
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

/// Split a body into deterministic, overlapping, UTF-8-safe passages.
///
/// A body at or below the target size produces one range. Longer bodies prefer
/// paragraph boundaries, then sentence boundaries, between 1,000 and 1,500
/// words. The fallback is 1,200 words. Adjacent ranges overlap by about 100
/// words.
#[must_use]
pub fn passage_ranges(body: &str) -> Vec<Passage> {
    let words = word_spans(body);
    passage_ranges_from_words(body, &words)
}

fn passage_ranges_from_words(body: &str, words: &[WordSpan]) -> Vec<Passage> {
    if words.is_empty() {
        return vec![Passage {
            start_byte: 0,
            end_byte: body.len(),
        }];
    }

    if words.len() <= TARGET_PASSAGE_WORDS {
        return vec![Passage {
            start_byte: 0,
            end_byte: body.len(),
        }];
    }

    let mut passages = Vec::new();
    let mut start_word = 0;
    while start_word < words.len() {
        let end_word = choose_passage_end(body, words, start_word);
        let start_byte = if start_word == 0 {
            0
        } else {
            words[start_word].start
        };
        let end_byte = if end_word == words.len() {
            body.len()
        } else {
            words[end_word].start
        };

        passages.push(Passage {
            start_byte,
            end_byte,
        });

        if end_word == words.len() {
            break;
        }
        start_word = end_word.saturating_sub(PASSAGE_OVERLAP_WORDS);
    }

    passages
}

/// Derive every search unit for one node.
///
/// `breadcrumb` must contain the root-to-node display path, including the
/// current node title.
///
/// # Errors
///
/// Returns an application error only if a body offset cannot fit in `SQLite`'s
/// signed integer representation.
pub fn derive_units(
    node_id: i64,
    title: &str,
    body: &str,
    breadcrumb: &str,
) -> Result<Vec<DerivedUnit>, AppError> {
    let normalized_title = normalize_key(title);
    let normalized_path = normalize_key(breadcrumb);
    let words = word_spans(body);
    let is_passage = words.len() > TARGET_PASSAGE_WORDS;
    let passages = passage_ranges(body);
    let mut units = Vec::with_capacity(passages.len());
    let mut unit_no = 0_i64;

    for passage in passages {
        let (unit_kind, start_byte, end_byte) = if is_passage {
            (
                UnitKind::Passage,
                Some(offset_to_i64(passage.start_byte)?),
                Some(offset_to_i64(passage.end_byte)?),
            )
        } else {
            (UnitKind::Node, None, None)
        };
        let text = passage.text(body).ok_or_else(|| {
            AppError::unexpected(
                "invalid_passage_range",
                "the indexer produced a passage outside the node body",
            )
        })?;
        let content_hash = unit_hash(&HashInputs {
            node_id,
            unit_no,
            unit_kind,
            title,
            normalized_title: &normalized_title,
            breadcrumb,
            normalized_path: &normalized_path,
            text,
            start_byte,
            end_byte,
            indexer_version: INDEXER_VERSION,
        });

        units.push(DerivedUnit {
            node_id,
            unit_no,
            unit_kind,
            title: title.to_owned(),
            normalized_title: normalized_title.clone(),
            breadcrumb: breadcrumb.to_owned(),
            normalized_path: normalized_path.clone(),
            text: text.to_owned(),
            start_byte,
            end_byte,
            content_hash,
            indexer_version: INDEXER_VERSION,
        });
        unit_no = unit_no.checked_add(1).ok_or_else(|| {
            AppError::unexpected(
                "too_many_search_units",
                "a node produced more search units than SQLite can address",
            )
        })?;
    }

    Ok(units)
}

/// Replace the derived units for one canonical node.
///
/// This function does not change the global index-version marker. It is suited
/// to ordinary edits performed while an otherwise-current index is maintained.
///
/// # Errors
///
/// Returns `node_not_found` if the node does not exist, or an index/database
/// error if derivation or persistence fails.
pub fn rebuild_node(transaction: &Transaction<'_>, node_id: i64) -> Result<usize, AppError> {
    let node = load_one_node(transaction, node_id)?;
    transaction
        .execute("DELETE FROM search_units WHERE node_id = ?1", [node_id])
        .map_err(|error| database_error("index_delete_failed", &error))?;
    insert_node_units(transaction, &node)
}

/// Replace all derived units for a node and its descendants.
///
/// This function does not change the global index-version marker.
///
/// # Errors
///
/// Returns `node_not_found` if the subtree root does not exist, or an
/// index/database error if derivation or persistence fails.
pub fn rebuild_subtree(
    transaction: &Transaction<'_>,
    root_id: i64,
) -> Result<RebuildStats, AppError> {
    let root_breadcrumb = breadcrumb_for_node(transaction, root_id)?;
    let nodes = load_subtree(transaction, root_id, &root_breadcrumb)?;
    if nodes.is_empty() {
        return Err(node_not_found(root_id));
    }

    rebuild_loaded_nodes(transaction, &nodes)
}

/// Replace all derived index state from canonical nodes.
///
/// The transaction remains owned by the caller. A later failure and rollback
/// therefore preserve the previous complete index.
///
/// # Errors
///
/// Returns an index/database error if canonical traversal, unit generation,
/// FTS rebuilding, or metadata maintenance fails.
pub fn rebuild_all(transaction: &Transaction<'_>) -> Result<RebuildStats, AppError> {
    let nodes = load_all_nodes(transaction)?;
    ensure_all_nodes_reachable(transaction, nodes.len())?;

    // Repair the external-content index from the existing unit rows first.
    // This makes the following trigger-driven deletes safe even when the FTS
    // postings had drifted from otherwise-valid search_units.
    transaction
        .execute("INSERT INTO search_fts(search_fts) VALUES ('rebuild')", [])
        .map_err(|error| database_error("fts_rebuild_failed", &error))?;

    transaction
        .execute("DELETE FROM search_units", [])
        .map_err(|error| database_error("index_clear_failed", &error))?;

    let mut stats = RebuildStats::default();
    for node in &nodes {
        stats.nodes += 1;
        stats.units += insert_node_units(transaction, node)?;
    }

    transaction
        .execute("INSERT INTO search_fts(search_fts) VALUES ('rebuild')", [])
        .map_err(|error| database_error("fts_rebuild_failed", &error))?;
    set_current_version(transaction)?;
    Ok(stats)
}

/// Inspect version metadata and cheap row-level completeness indicators.
///
/// # Errors
///
/// Returns a database error if index metadata or unit counts cannot be read.
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

    let missing_units = connection
        .query_row(
            "SELECT COUNT(*) \
             FROM nodes AS n \
             WHERE NOT EXISTS (\
                 SELECT 1 FROM search_units AS su WHERE su.node_id = n.id\
             )",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| database_error("index_status_failed", &error))?;
    if missing_units != 0 {
        return Ok(Status::MissingUnits {
            node_count: missing_units,
        });
    }

    Ok(Status::Current)
}

/// Reject search against missing, stale, or incomplete derived data.
///
/// # Errors
///
/// Returns `reindex_required` when [`status`] is not current, or forwards a
/// database error from the status check.
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

/// Store the current indexer version in an existing transaction.
///
/// Prefer [`rebuild_all`] unless the caller has independently recreated every
/// derived row.
///
/// # Errors
///
/// Returns a database error if metadata cannot be written.
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

fn word_spans(body: &str) -> Vec<WordSpan> {
    let mut words = Vec::new();
    let mut word_start = None;

    for (byte, character) in body.char_indices() {
        if character.is_whitespace() {
            if let Some(start) = word_start.take() {
                words.push(WordSpan { start, end: byte });
            }
        } else if word_start.is_none() {
            word_start = Some(byte);
        }
    }
    if let Some(start) = word_start {
        words.push(WordSpan {
            start,
            end: body.len(),
        });
    }

    words
}

fn choose_passage_end(body: &str, words: &[WordSpan], start_word: usize) -> usize {
    let remaining = words.len() - start_word;
    if remaining <= MAX_PASSAGE_WORDS {
        return words.len();
    }

    let minimum = start_word + MIN_NATURAL_PASSAGE_WORDS;
    let target = start_word + TARGET_PASSAGE_WORDS;
    let maximum = start_word + MAX_PASSAGE_WORDS;

    nearest_boundary(body, words, minimum..=maximum, target, is_paragraph_gap)
        .or_else(|| nearest_boundary(body, words, minimum..=maximum, target, is_sentence_gap))
        .unwrap_or(target)
}

fn nearest_boundary(
    body: &str,
    words: &[WordSpan],
    candidates: std::ops::RangeInclusive<usize>,
    target: usize,
    predicate: fn(&str, &str, &WordSpan) -> bool,
) -> Option<usize> {
    candidates
        .filter(|&end_word| {
            let previous = &words[end_word - 1];
            let next = &words[end_word];
            body.get(previous.end..next.start)
                .is_some_and(|gap| predicate(body, gap, previous))
        })
        .min_by_key(|&end_word| (end_word.abs_diff(target), end_word))
}

fn is_paragraph_gap(_body: &str, gap: &str, _previous: &WordSpan) -> bool {
    gap.chars().filter(|&character| character == '\n').count() >= 2
}

fn is_sentence_gap(body: &str, gap: &str, previous: &WordSpan) -> bool {
    if gap.is_empty() {
        return false;
    }
    body.get(previous.start..previous.end)
        .is_some_and(sentence_boundary)
}

fn sentence_boundary(word: &str) -> bool {
    matches!(
        word.trim_end_matches(['"', '\'', ')', ']', '}'])
            .chars()
            .next_back(),
        Some('.' | '!' | '?')
    )
}

fn offset_to_i64(offset: usize) -> Result<i64, AppError> {
    i64::try_from(offset).map_err(|_| {
        AppError::unexpected(
            "body_too_large",
            "a node body is too large for SQLite byte offsets",
        )
    })
}

struct HashInputs<'a> {
    node_id: i64,
    unit_no: i64,
    unit_kind: UnitKind,
    title: &'a str,
    normalized_title: &'a str,
    breadcrumb: &'a str,
    normalized_path: &'a str,
    text: &'a str,
    start_byte: Option<i64>,
    end_byte: Option<i64>,
    indexer_version: i64,
}

fn unit_hash(inputs: &HashInputs<'_>) -> String {
    let mut hasher = Sha256::new();
    hash_i64(&mut hasher, inputs.node_id);
    hash_i64(&mut hasher, inputs.unit_no);
    hash_text(&mut hasher, inputs.unit_kind.as_str());
    hash_text(&mut hasher, inputs.title);
    hash_text(&mut hasher, inputs.normalized_title);
    hash_text(&mut hasher, inputs.breadcrumb);
    hash_text(&mut hasher, inputs.normalized_path);
    hash_text(&mut hasher, inputs.text);
    hash_optional_i64(&mut hasher, inputs.start_byte);
    hash_optional_i64(&mut hasher, inputs.end_byte);
    hash_i64(&mut hasher, inputs.indexer_version);
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

fn hash_i64(hasher: &mut Sha256, value: i64) {
    hasher.update(value.to_be_bytes());
}

fn hash_optional_i64(hasher: &mut Sha256, value: Option<i64>) {
    match value {
        Some(value) => {
            hasher.update([1]);
            hash_i64(hasher, value);
        }
        None => hasher.update([0]),
    }
}

fn load_one_node(transaction: &Transaction<'_>, node_id: i64) -> Result<NodeText, AppError> {
    let (title, body) = transaction
        .query_row(
            "SELECT title, body FROM nodes WHERE id = ?1",
            [node_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| database_error("index_node_read_failed", &error))?
        .ok_or_else(|| node_not_found(node_id))?;
    let breadcrumb = breadcrumb_for_node(transaction, node_id)?;
    Ok(NodeText {
        id: node_id,
        title,
        body,
        breadcrumb,
    })
}

fn breadcrumb_for_node(transaction: &Transaction<'_>, node_id: i64) -> Result<String, AppError> {
    let mut statement = transaction
        .prepare(
            "WITH RECURSIVE ancestors(id, parent_id, title, depth, visited) AS (\
                 SELECT id, parent_id, title, 0, ',' || id || ',' \
                 FROM nodes WHERE id = ?1 \
                 UNION ALL \
                 SELECT parent.id, parent.parent_id, parent.title, ancestors.depth + 1, \
                        ancestors.visited || parent.id || ',' \
                 FROM nodes AS parent \
                 JOIN ancestors ON parent.id = ancestors.parent_id \
                 WHERE instr(ancestors.visited, ',' || parent.id || ',') = 0\
             ) \
             SELECT title FROM ancestors ORDER BY depth DESC",
        )
        .map_err(|error| database_error("breadcrumb_read_failed", &error))?;
    let titles = statement
        .query_map([node_id], |row| row.get::<_, String>(0))
        .map_err(|error| database_error("breadcrumb_read_failed", &error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| database_error("breadcrumb_read_failed", &error))?;

    if titles.is_empty() {
        return Err(node_not_found(node_id));
    }
    Ok(titles.join(BREADCRUMB_SEPARATOR))
}

fn load_subtree(
    transaction: &Transaction<'_>,
    root_id: i64,
    root_breadcrumb: &str,
) -> Result<Vec<NodeText>, AppError> {
    let mut statement = transaction
        .prepare(
            "WITH RECURSIVE subtree(id, parent_id, title, body, breadcrumb, sort_path) AS (\
                 SELECT id, parent_id, title, body, ?2, '' \
                 FROM nodes WHERE id = ?1 \
                 UNION ALL \
                 SELECT child.id, child.parent_id, child.title, child.body, \
                        subtree.breadcrumb || ' / ' || child.title, \
                        subtree.sort_path || '/' || printf('%020d:%020d', child.position, child.id) \
                 FROM nodes AS child \
                 JOIN subtree ON child.parent_id = subtree.id\
             ) \
             SELECT id, title, body, breadcrumb FROM subtree ORDER BY sort_path",
        )
        .map_err(|error| database_error("subtree_index_read_failed", &error))?;
    statement
        .query_map(params![root_id, root_breadcrumb], node_text_from_row)
        .map_err(|error| database_error("subtree_index_read_failed", &error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| database_error("subtree_index_read_failed", &error))
}

fn load_all_nodes(transaction: &Transaction<'_>) -> Result<Vec<NodeText>, AppError> {
    let mut statement = transaction
        .prepare(
            "WITH RECURSIVE forest(id, parent_id, title, body, breadcrumb, sort_path) AS (\
                 SELECT id, parent_id, title, body, title, \
                        printf('%020d:%020d', position, id) \
                 FROM nodes WHERE parent_id IS NULL \
                 UNION ALL \
                 SELECT child.id, child.parent_id, child.title, child.body, \
                        forest.breadcrumb || ' / ' || child.title, \
                        forest.sort_path || '/' || printf('%020d:%020d', child.position, child.id) \
                 FROM nodes AS child \
                 JOIN forest ON child.parent_id = forest.id\
             ) \
             SELECT id, title, body, breadcrumb FROM forest ORDER BY sort_path",
        )
        .map_err(|error| database_error("index_source_read_failed", &error))?;
    statement
        .query_map([], node_text_from_row)
        .map_err(|error| database_error("index_source_read_failed", &error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| database_error("index_source_read_failed", &error))
}

fn node_text_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NodeText> {
    Ok(NodeText {
        id: row.get(0)?,
        title: row.get(1)?,
        body: row.get(2)?,
        breadcrumb: row.get(3)?,
    })
}

fn ensure_all_nodes_reachable(
    transaction: &Transaction<'_>,
    reachable_count: usize,
) -> Result<(), AppError> {
    let canonical_count = transaction
        .query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get::<_, i64>(0))
        .map_err(|error| database_error("index_source_read_failed", &error))?;
    let reachable_count = i64::try_from(reachable_count).map_err(|_| {
        AppError::unexpected(
            "too_many_nodes",
            "the library has more nodes than SQLite can address",
        )
    })?;
    if reachable_count != canonical_count {
        return Err(AppError::database(
            "invalid_tree_for_reindex",
            "not every canonical node is reachable from a root; run `annals validate`",
        ));
    }
    Ok(())
}

fn rebuild_loaded_nodes(
    transaction: &Transaction<'_>,
    nodes: &[NodeText],
) -> Result<RebuildStats, AppError> {
    let mut stats = RebuildStats::default();
    for node in nodes {
        transaction
            .execute("DELETE FROM search_units WHERE node_id = ?1", [node.id])
            .map_err(|error| database_error("index_delete_failed", &error))?;
        stats.nodes += 1;
        stats.units += insert_node_units(transaction, node)?;
    }
    Ok(stats)
}

fn insert_node_units(transaction: &Transaction<'_>, node: &NodeText) -> Result<usize, AppError> {
    let units = derive_units(node.id, &node.title, &node.body, &node.breadcrumb)?;
    let mut statement = transaction
        .prepare_cached(
            "INSERT INTO search_units(\
                 node_id, unit_no, unit_kind, title, normalized_title, breadcrumb, \
                 normalized_path, text, start_byte, end_byte, content_hash, indexer_version\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )
        .map_err(|error| database_error("index_insert_failed", &error))?;

    for unit in &units {
        statement
            .execute(params![
                unit.node_id,
                unit.unit_no,
                unit.unit_kind.as_str(),
                unit.title,
                unit.normalized_title,
                unit.breadcrumb,
                unit.normalized_path,
                unit.text,
                unit.start_byte,
                unit.end_byte,
                unit.content_hash,
                unit.indexer_version,
            ])
            .map_err(|error| database_error("index_insert_failed", &error))?;
    }
    Ok(units.len())
}

fn node_not_found(node_id: i64) -> AppError {
    AppError::not_found(
        "node_not_found",
        format!("node {node_id} was not found while rebuilding the search index"),
    )
}

fn database_error(code: &'static str, error: &rusqlite::Error) -> AppError {
    AppError::database(
        code,
        format!("search index database operation failed: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use rusqlite::Connection;

    use super::{
        Passage, Status, derive_units, normalize_key, passage_ranges, rebuild_all, rebuild_subtree,
        status,
    };

    #[test]
    fn normalization_is_nfkc_lowercase_and_whitespace_stable() {
        assert_eq!(
            normalize_key("  Ｃ＋＋\u{2003}Index\tAPI  "),
            "c++ index api"
        );
        assert_eq!(normalize_key("C#"), "c#");
        assert_ne!(normalize_key("C#"), normalize_key("C++"));
    }

    #[test]
    fn short_and_empty_bodies_make_node_units() -> Result<(), Box<dyn Error>> {
        let empty = derive_units(7, " Empty ", "", "Root /  Empty ")?;
        assert_eq!(empty.len(), 1);
        let Some(unit) = empty.first() else {
            return Err(std::io::Error::other("missing title-only unit").into());
        };
        assert_eq!(unit.unit_kind.as_str(), "node");
        assert_eq!(unit.start_byte, None);
        assert_eq!(unit.normalized_title, "empty");
        assert!(unit.content_hash.starts_with("sha256:"));
        assert_eq!(unit.content_hash.len(), 71);
        Ok(())
    }

    #[test]
    fn passages_are_bounded_overlapping_and_utf8_safe() -> Result<(), Box<dyn Error>> {
        let body = make_words(2_500, "λ");
        let passages = passage_ranges(&body);
        assert!(passages.len() >= 2);

        for passage in &passages {
            let Some(text) = passage.text(&body) else {
                return Err(std::io::Error::other("passage was not UTF-8 safe").into());
            };
            assert!(text.split_whitespace().count() <= 1_500);
        }

        for pair in passages.windows(2) {
            let Some(first) = pair.first() else {
                return Err(std::io::Error::other("missing first passage").into());
            };
            let Some(second) = pair.get(1) else {
                return Err(std::io::Error::other("missing second passage").into());
            };
            assert!(second.start_byte < first.end_byte);
        }
        Ok(())
    }

    #[test]
    fn paragraph_boundary_is_preferred() -> Result<(), Box<dyn Error>> {
        let first = make_words(1_100, "a");
        let second = make_words(600, "b");
        let body = format!("{first}\n\n{second}");
        let passages = passage_ranges(&body);
        let Some(Passage { end_byte, .. }) = passages.first() else {
            return Err(std::io::Error::other("missing first passage").into());
        };
        assert_eq!(*end_byte, first.len() + 2);
        Ok(())
    }

    #[test]
    fn hashing_changes_with_row_inputs() -> Result<(), Box<dyn Error>> {
        let first = derive_units(1, "Title", "body", "Root / Title")?;
        let second = derive_units(2, "Title", "body", "Root / Title")?;
        let Some(first) = first.first() else {
            return Err(std::io::Error::other("missing first unit").into());
        };
        let Some(second) = second.first() else {
            return Err(std::io::Error::other("missing second unit").into());
        };
        assert_ne!(first.content_hash, second.content_hash);
        Ok(())
    }

    #[test]
    fn full_and_subtree_rebuilds_maintain_metadata_and_breadcrumbs() -> Result<(), Box<dyn Error>> {
        let mut connection = test_connection()?;
        connection.execute(
            "INSERT INTO nodes(id, parent_id, title, body, position) \
             VALUES (1, NULL, 'Root', 'root text', 0), \
                    (2, 1, 'Child', 'child text', 0)",
            [],
        )?;

        {
            let transaction = connection.transaction()?;
            let stats = rebuild_all(&transaction)?;
            assert_eq!(stats.nodes, 2);
            assert_eq!(stats.units, 2);
            transaction.commit()?;
        }
        assert_eq!(status(&connection)?, Status::Current);

        connection.execute("UPDATE nodes SET title = 'Renamed' WHERE id = 1", [])?;
        {
            let transaction = connection.transaction()?;
            let stats = rebuild_subtree(&transaction, 1)?;
            assert_eq!(stats.nodes, 2);
            transaction.commit()?;
        }

        let breadcrumb: String = connection.query_row(
            "SELECT breadcrumb FROM search_units WHERE node_id = 2",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(breadcrumb, "Renamed / Child");
        assert_eq!(status(&connection)?, Status::Current);
        Ok(())
    }

    fn make_words(count: usize, word: &str) -> String {
        std::iter::repeat_n(word, count)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn test_connection() -> Result<Connection, rusqlite::Error> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON; \
             CREATE TABLE nodes(\
                 id INTEGER PRIMARY KEY, \
                 parent_id INTEGER REFERENCES nodes(id) ON DELETE CASCADE, \
                 title TEXT NOT NULL, \
                 body TEXT NOT NULL, \
                 position INTEGER NOT NULL\
             ); \
             CREATE TABLE search_units(\
                 id INTEGER PRIMARY KEY, \
                 node_id INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE, \
                 unit_no INTEGER NOT NULL, \
                 unit_kind TEXT NOT NULL, \
                 title TEXT NOT NULL, \
                 normalized_title TEXT NOT NULL, \
                 breadcrumb TEXT NOT NULL, \
                 normalized_path TEXT NOT NULL, \
                 text TEXT NOT NULL, \
                 start_byte INTEGER, \
                 end_byte INTEGER, \
                 content_hash TEXT NOT NULL, \
                 indexer_version INTEGER NOT NULL, \
                 UNIQUE(node_id, unit_no)\
             ); \
             CREATE VIRTUAL TABLE search_fts USING fts5(\
                 title, breadcrumb, text, \
                 content = 'search_units', content_rowid = 'id'\
             ); \
             CREATE TRIGGER search_units_after_insert AFTER INSERT ON search_units BEGIN \
                 INSERT INTO search_fts(rowid, title, breadcrumb, text) \
                 VALUES (new.id, new.title, new.breadcrumb, new.text); \
             END; \
             CREATE TRIGGER search_units_after_delete AFTER DELETE ON search_units BEGIN \
                 INSERT INTO search_fts(search_fts, rowid, title, breadcrumb, text) \
                 VALUES ('delete', old.id, old.title, old.breadcrumb, old.text); \
             END; \
             CREATE TABLE index_metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        Ok(connection)
    }
}
