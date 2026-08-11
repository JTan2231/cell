use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use crate::error::AppError;
use crate::index::{self, DerivedUnit};
use crate::model::{ValidationIssue, ValidationReport, ValidationSeverity};

#[derive(Debug)]
struct CanonicalNode {
    id: i64,
    parent_id: Option<i64>,
    kind: String,
    title: String,
    body: String,
    position: i64,
}

#[derive(Debug)]
struct StoredUnit {
    node_id: i64,
    unit_no: i64,
    unit_kind: String,
    title: String,
    normalized_title: String,
    breadcrumb: String,
    normalized_path: String,
    text: String,
    start_byte: Option<i64>,
    end_byte: Option<i64>,
    content_hash: String,
    indexer_version: i64,
}

/// Validate canonical tree state, `SQLite` integrity, and the derived index.
///
/// Validation is read-only. Structural and index problems are returned as
/// findings; a failure to run a validation query is returned as an application
/// error.
///
/// # Errors
///
/// Returns a database or indexing error if the checks cannot be completed.
pub fn validate(connection: &Connection) -> Result<ValidationReport, AppError> {
    let mut issues = Vec::new();
    check_sqlite_integrity(connection, &mut issues)?;
    check_foreign_keys(connection, &mut issues)?;

    let nodes = load_canonical_nodes(connection)?;
    let source_rows = load_source_rows(connection)?;
    check_tree_and_source_invariants(&nodes, &source_rows, &mut issues);
    check_derived_index(connection, &nodes, &mut issues)?;
    check_fts_integrity(connection, &mut issues);

    let valid = !issues
        .iter()
        .any(|issue| issue.severity == ValidationSeverity::Error);
    Ok(ValidationReport { valid, issues })
}

fn check_fts_integrity(connection: &Connection, issues: &mut Vec<ValidationIssue>) {
    if let Err(error) = connection.execute(
        "INSERT INTO search_fts(search_fts, rank) VALUES('integrity-check', 1)",
        [],
    ) {
        issues.push(error_issue(
            "fts_integrity_error",
            format!("the full-text index is inconsistent with search units: {error}"),
            None,
        ));
    }
}

fn check_sqlite_integrity(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement = connection
        .prepare("PRAGMA integrity_check")
        .map_err(|error| validation_error("prepare integrity check", &error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| validation_error("run integrity check", &error))?;
    for row in rows {
        let message = row.map_err(|error| validation_error("read integrity result", &error))?;
        if message != "ok" {
            issues.push(error_issue("sqlite_integrity_error", message, None));
        }
    }
    Ok(())
}

fn check_foreign_keys(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(|error| validation_error("prepare foreign-key check", &error))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .map_err(|error| validation_error("run foreign-key check", &error))?;
    for row in rows {
        let (table, row_id, parent, foreign_key) =
            row.map_err(|error| validation_error("read foreign-key result", &error))?;
        issues.push(error_issue(
            "foreign_key_violation",
            format!("table {table} row {row_id:?} violates foreign key {foreign_key} to {parent}"),
            row_id,
        ));
    }
    Ok(())
}

fn load_canonical_nodes(connection: &Connection) -> Result<BTreeMap<i64, CanonicalNode>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT id, parent_id, kind, title, body, position \
             FROM nodes ORDER BY id",
        )
        .map_err(|error| validation_error("prepare node validation", &error))?;
    let rows = statement
        .query_map([], |row| {
            Ok(CanonicalNode {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                kind: row.get(2)?,
                title: row.get(3)?,
                body: row.get(4)?,
                position: row.get(5)?,
            })
        })
        .map_err(|error| validation_error("run node validation", &error))?;
    rows.map(|row| {
        let node = row.map_err(|error| validation_error("read node validation row", &error))?;
        Ok((node.id, node))
    })
    .collect()
}

fn load_source_rows(connection: &Connection) -> Result<BTreeSet<i64>, AppError> {
    let mut statement = connection
        .prepare("SELECT node_id FROM sources ORDER BY node_id")
        .map_err(|error| validation_error("prepare source validation", &error))?;
    let rows = statement
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|error| validation_error("run source validation", &error))?;
    rows.map(|row| row.map_err(|error| validation_error("read source validation row", &error)))
        .collect()
}

fn check_tree_and_source_invariants(
    nodes: &BTreeMap<i64, CanonicalNode>,
    source_rows: &BTreeSet<i64>,
    issues: &mut Vec<ValidationIssue>,
) {
    let parent_ids = nodes
        .values()
        .filter_map(|node| node.parent_id)
        .collect::<BTreeSet<_>>();

    check_parent_links_and_cycles(nodes, issues);
    check_positions(nodes, issues);

    for node in nodes.values() {
        match node.kind.as_str() {
            "source" => {
                if parent_ids.contains(&node.id) {
                    issues.push(error_issue(
                        "source_has_children",
                        format!("source node {} has one or more children", node.id),
                        Some(node.id),
                    ));
                }
                if !source_rows.contains(&node.id) {
                    issues.push(error_issue(
                        "source_metadata_missing",
                        format!("source node {} has no source metadata row", node.id),
                        Some(node.id),
                    ));
                }
            }
            "topic" => {
                if source_rows.contains(&node.id) {
                    issues.push(error_issue(
                        "source_metadata_kind_mismatch",
                        format!("topic node {} has a source metadata row", node.id),
                        Some(node.id),
                    ));
                }
                if !parent_ids.contains(&node.id) {
                    issues.push(warning_issue(
                        "incomplete_topic_leaf",
                        format!("topic node {} is a leaf", node.id),
                        Some(node.id),
                    ));
                }
            }
            invalid => issues.push(error_issue(
                "invalid_node_kind",
                format!("node {} has invalid kind {invalid:?}", node.id),
                Some(node.id),
            )),
        }
    }

    for source_id in source_rows {
        if !nodes.contains_key(source_id) {
            issues.push(error_issue(
                "orphan_source_metadata",
                format!("source metadata refers to missing node {source_id}"),
                Some(*source_id),
            ));
        }
    }
}

fn check_parent_links_and_cycles(
    nodes: &BTreeMap<i64, CanonicalNode>,
    issues: &mut Vec<ValidationIssue>,
) {
    for node in nodes.values() {
        if let Some(parent_id) = node.parent_id
            && !nodes.contains_key(&parent_id)
        {
            issues.push(error_issue(
                "dangling_parent",
                format!("node {} refers to missing parent {parent_id}", node.id),
                Some(node.id),
            ));
        }
    }

    let mut cycle_members = BTreeSet::new();
    for node in nodes.values() {
        let mut path = Vec::new();
        let mut positions = BTreeMap::new();
        let mut current = Some(node.id);
        while let Some(node_id) = current {
            if let Some(&cycle_start) = positions.get(&node_id) {
                cycle_members.extend(path[cycle_start..].iter().copied());
                break;
            }
            positions.insert(node_id, path.len());
            path.push(node_id);
            current = nodes.get(&node_id).and_then(|entry| entry.parent_id);
        }
    }
    for node_id in cycle_members {
        issues.push(error_issue(
            "cycle_detected",
            format!("node {node_id} belongs to a parent cycle"),
            Some(node_id),
        ));
    }
}

fn check_positions(nodes: &BTreeMap<i64, CanonicalNode>, issues: &mut Vec<ValidationIssue>) {
    let mut positions = BTreeMap::new();
    let mut duplicate_nodes = BTreeSet::new();
    for node in nodes.values() {
        if node.position < 0 {
            issues.push(error_issue(
                "invalid_sibling_position",
                format!(
                    "node {} has negative sibling position {}",
                    node.id, node.position
                ),
                Some(node.id),
            ));
        }
        let key = (node.parent_id, node.position);
        if let Some(previous) = positions.insert(key, node.id) {
            duplicate_nodes.insert(previous);
            duplicate_nodes.insert(node.id);
        }
    }
    for node_id in duplicate_nodes {
        issues.push(error_issue(
            "duplicate_sibling_position",
            format!("node {node_id} shares its sibling position with another node"),
            Some(node_id),
        ));
    }
}

fn check_derived_index(
    connection: &Connection,
    nodes: &BTreeMap<i64, CanonicalNode>,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let status = index::status(connection)?;
    if !status.is_current() {
        issues.push(error_issue(
            "index_not_current",
            format!("derived search index is not current: {status:?}"),
            None,
        ));
    }

    let mut expected = BTreeMap::new();
    for node in nodes.values() {
        let Some(breadcrumb) = breadcrumb_for(node.id, nodes) else {
            continue;
        };
        for unit in index::derive_units(node.id, &node.title, &node.body, &breadcrumb)? {
            expected.insert((unit.node_id, unit.unit_no), unit);
        }
    }

    let stored_units = load_stored_units(connection)?;
    check_stored_ranges(&stored_units, nodes, issues);
    compare_units(expected, stored_units, issues);
    Ok(())
}

fn breadcrumb_for(node_id: i64, nodes: &BTreeMap<i64, CanonicalNode>) -> Option<String> {
    let mut titles = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = Some(node_id);
    while let Some(id) = current {
        if !seen.insert(id) {
            return None;
        }
        let node = nodes.get(&id)?;
        titles.push(node.title.as_str());
        current = node.parent_id;
    }
    titles.reverse();
    Some(titles.join(index::BREADCRUMB_SEPARATOR))
}

fn load_stored_units(connection: &Connection) -> Result<Vec<StoredUnit>, AppError> {
    let mut statement = connection
        .prepare(
            "SELECT node_id, unit_no, unit_kind, title, normalized_title, \
                    breadcrumb, normalized_path, text, start_byte, end_byte, \
                    content_hash, indexer_version \
             FROM search_units ORDER BY node_id, unit_no",
        )
        .map_err(|error| validation_error("prepare search-unit validation", &error))?;
    let rows = statement
        .query_map([], |row| {
            Ok(StoredUnit {
                node_id: row.get(0)?,
                unit_no: row.get(1)?,
                unit_kind: row.get(2)?,
                title: row.get(3)?,
                normalized_title: row.get(4)?,
                breadcrumb: row.get(5)?,
                normalized_path: row.get(6)?,
                text: row.get(7)?,
                start_byte: row.get(8)?,
                end_byte: row.get(9)?,
                content_hash: row.get(10)?,
                indexer_version: row.get(11)?,
            })
        })
        .map_err(|error| validation_error("run search-unit validation", &error))?;
    rows.map(|row| row.map_err(|error| validation_error("read search-unit validation row", &error)))
        .collect()
}

fn check_stored_ranges(
    units: &[StoredUnit],
    nodes: &BTreeMap<i64, CanonicalNode>,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut invalid_nodes = BTreeSet::new();
    for unit in units {
        let valid = nodes
            .get(&unit.node_id)
            .is_some_and(|node| stored_range_is_valid(unit, &node.body));
        if !valid {
            invalid_nodes.insert(unit.node_id);
        }
    }
    for node_id in invalid_nodes {
        issues.push(error_issue(
            "invalid_search_unit_range",
            format!("node {node_id} has a search unit with an invalid body range"),
            Some(node_id),
        ));
    }
}

fn stored_range_is_valid(unit: &StoredUnit, body: &str) -> bool {
    match (unit.unit_kind.as_str(), unit.start_byte, unit.end_byte) {
        ("node", None, None) => true,
        ("passage", Some(start), Some(end)) if start >= 0 && end >= start => {
            let Ok(start) = usize::try_from(start) else {
                return false;
            };
            let Ok(end) = usize::try_from(end) else {
                return false;
            };
            body.get(start..end).is_some()
        }
        _ => false,
    }
}

fn compare_units(
    expected: BTreeMap<(i64, i64), DerivedUnit>,
    stored: Vec<StoredUnit>,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut stored = stored
        .into_iter()
        .map(|unit| ((unit.node_id, unit.unit_no), unit))
        .collect::<BTreeMap<_, _>>();
    let mut missing_nodes = BTreeSet::new();
    let mut stale_nodes = BTreeSet::new();

    for (key, expected_unit) in expected {
        match stored.remove(&key) {
            Some(stored_unit) if unit_matches(&stored_unit, &expected_unit) => {}
            Some(_) => {
                stale_nodes.insert(key.0);
            }
            None => {
                missing_nodes.insert(key.0);
            }
        }
    }

    for node_id in missing_nodes {
        issues.push(error_issue(
            "search_unit_missing",
            format!("node {node_id} is missing one or more derived search units"),
            Some(node_id),
        ));
    }
    for node_id in stale_nodes {
        issues.push(error_issue(
            "search_unit_stale",
            format!("node {node_id} has stale or inconsistent search units"),
            Some(node_id),
        ));
    }

    let unexpected_nodes = stored
        .into_values()
        .map(|unit| unit.node_id)
        .collect::<BTreeSet<_>>();
    for node_id in unexpected_nodes {
        issues.push(error_issue(
            "search_unit_unexpected",
            format!("node {node_id} has unexpected derived search units"),
            Some(node_id),
        ));
    }
}

fn unit_matches(stored: &StoredUnit, expected: &DerivedUnit) -> bool {
    stored.node_id == expected.node_id
        && stored.unit_no == expected.unit_no
        && stored.unit_kind == expected.unit_kind.as_str()
        && stored.title == expected.title
        && stored.normalized_title == expected.normalized_title
        && stored.breadcrumb == expected.breadcrumb
        && stored.normalized_path == expected.normalized_path
        && stored.text == expected.text
        && stored.start_byte == expected.start_byte
        && stored.end_byte == expected.end_byte
        && stored.content_hash == expected.content_hash
        && stored.indexer_version == expected.indexer_version
}

fn error_issue(code: &str, message: impl Into<String>, node_id: Option<i64>) -> ValidationIssue {
    ValidationIssue {
        severity: ValidationSeverity::Error,
        code: code.to_owned(),
        message: message.into(),
        node_id,
    }
}

fn warning_issue(code: &str, message: impl Into<String>, node_id: Option<i64>) -> ValidationIssue {
    ValidationIssue {
        severity: ValidationSeverity::Warning,
        code: code.to_owned(),
        message: message.into(),
        node_id,
    }
}

fn validation_error(action: &str, error: &rusqlite::Error) -> AppError {
    AppError::database("validation_failed", format!("unable to {action}: {error}"))
}
