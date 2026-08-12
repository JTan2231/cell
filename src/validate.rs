use std::collections::{BTreeMap, BTreeSet, HashSet};

use rusqlite::Connection;

use crate::error::AppError;
use crate::generation::{self, GeneratedTree, RawUnit, ResolutionPolicy};
use crate::index;
use crate::model::{ValidationIssue, ValidationReport, ValidationSeverity};

#[derive(Debug)]
struct CanonicalNode {
    id: i64,
    parent_id: Option<i64>,
    generation_run_id: Option<i64>,
    text: String,
    position: i64,
}

#[derive(Debug)]
struct StoredGeneration {
    id: i64,
    root_id: Option<i64>,
    raw_text: String,
    raw_sha256: String,
    adapter_name: String,
    adapter_version: String,
    proposal_json: String,
    policy: ResolutionPolicy,
}

/// Validate canonical, grounding, `SQLite`, and derived-index invariants without repair.
pub fn validate(connection: &Connection) -> Result<ValidationReport, AppError> {
    let mut issues = Vec::new();
    check_sqlite_integrity(connection, &mut issues)?;
    check_foreign_keys(connection, &mut issues)?;
    check_fts_integrity(connection, &mut issues);
    let nodes = load_nodes(connection)?;
    check_trees(&nodes, &mut issues);
    check_generation_grounding(connection, &nodes, &mut issues)?;
    check_index(connection, &nodes, &mut issues)?;
    Ok(ValidationReport {
        valid: !issues
            .iter()
            .any(|issue| issue.severity == ValidationSeverity::Error),
        issues,
    })
}

fn check_sqlite_integrity(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement = connection.prepare("PRAGMA integrity_check")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows {
        let result = row?;
        if result != "ok" {
            issues.push(error_issue(
                "sqlite_integrity",
                format!("SQLite integrity check reported: {result}"),
                None,
            ));
        }
    }
    Ok(())
}

fn check_foreign_keys(
    connection: &Connection,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (table, row_id, parent) = row?;
        issues.push(error_issue(
            "foreign_key_violation",
            format!("{table} row {row_id:?} has an invalid reference to {parent}"),
            None,
        ));
    }
    Ok(())
}

fn check_fts_integrity(connection: &Connection, issues: &mut Vec<ValidationIssue>) {
    if let Err(error) = connection.execute(
        "INSERT INTO search_fts(search_fts, rank) VALUES ('integrity-check', 1)",
        [],
    ) {
        issues.push(error_issue(
            "fts_integrity",
            format!("FTS integrity check failed: {error}"),
            None,
        ));
    }
}

fn load_nodes(connection: &Connection) -> Result<BTreeMap<i64, CanonicalNode>, AppError> {
    let mut statement = connection.prepare(
        "SELECT id, parent_id, generation_run_id, text, position FROM nodes ORDER BY id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(CanonicalNode {
            id: row.get(0)?,
            parent_id: row.get(1)?,
            generation_run_id: row.get(2)?,
            text: row.get(3)?,
            position: row.get(4)?,
        })
    })?;
    rows.map(|row| row.map(|node| (node.id, node)))
        .collect::<Result<_, _>>()
        .map_err(AppError::from)
}

fn check_trees(nodes: &BTreeMap<i64, CanonicalNode>, issues: &mut Vec<ValidationIssue>) {
    for node in nodes.values() {
        if node.text.is_empty() || node.text.trim() != node.text {
            issues.push(error_issue(
                "invalid_node_text",
                "node text must be nonempty and trimmed",
                Some(node.id),
            ));
        }
        if let Some(parent_id) = node.parent_id
            && !nodes.contains_key(&parent_id)
        {
            issues.push(error_issue(
                "missing_parent",
                format!("parent node {parent_id} does not exist"),
                Some(node.id),
            ));
        }

        let mut seen = HashSet::new();
        let mut current = Some(node.id);
        while let Some(id) = current {
            if !seen.insert(id) {
                issues.push(error_issue(
                    "tree_cycle",
                    "parent links contain a cycle",
                    Some(node.id),
                ));
                break;
            }
            current = nodes.get(&id).and_then(|entry| entry.parent_id);
        }
    }

    let mut sibling_positions = BTreeMap::<Option<i64>, Vec<(i64, i64)>>::new();
    for node in nodes.values() {
        sibling_positions
            .entry(node.parent_id)
            .or_default()
            .push((node.position, node.id));
    }
    for (parent, mut positions) in sibling_positions {
        positions.sort_unstable();
        for pair in positions.windows(2) {
            if pair[0].0 >= pair[1].0 {
                issues.push(error_issue(
                    "invalid_position",
                    format!("siblings under {parent:?} must have increasing positions"),
                    Some(pair[1].1),
                ));
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn check_generation_grounding(
    connection: &Connection,
    nodes: &BTreeMap<i64, CanonicalNode>,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let mut statement = connection.prepare(
        "SELECT gr.id, gr.root_node_id, ri.text, ri.sha256, \
                gr.adapter_name, gr.adapter_version, gr.accepted_proposal_json, \
                gr.node_budget, gr.max_depth, gr.max_children \
         FROM generation_runs AS gr JOIN raw_inputs AS ri ON ri.id = gr.input_id \
         ORDER BY gr.id",
    )?;
    let runs = statement
        .query_map([], |row| {
            Ok(StoredGeneration {
                id: row.get(0)?,
                root_id: row.get(1)?,
                raw_text: row.get(2)?,
                raw_sha256: row.get(3)?,
                adapter_name: row.get(4)?,
                adapter_version: row.get(5)?,
                proposal_json: row.get(6)?,
                policy: ResolutionPolicy {
                    node_budget: row.get(7)?,
                    max_depth: row.get(8)?,
                    max_children: row.get(9)?,
                },
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for run in runs {
        let run_id = run.id;
        let actual_sha256 = sha256_hex(run.raw_text.as_bytes());
        if actual_sha256 != run.raw_sha256 {
            issues.push(error_issue(
                "raw_input_checksum_mismatch",
                format!("generation run {run_id} raw input checksum is invalid"),
                None,
            ));
        }
        let Some(root_id) = run.root_id else {
            issues.push(error_issue(
                "generation_root_missing",
                format!("generation run {run_id} has no root node"),
                None,
            ));
            continue;
        };
        if nodes.get(&root_id).and_then(|node| node.generation_run_id) != Some(run_id) {
            issues.push(error_issue(
                "generation_root_mismatch",
                format!("generation run {run_id} does not own root {root_id}"),
                Some(root_id),
            ));
        }

        let mut units = connection.prepare(
            "SELECT unit_id, start_byte, end_byte FROM input_units \
             WHERE run_id = ?1 ORDER BY start_byte",
        )?;
        let ranges = units
            .query_map([run_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut expected_start = 0_usize;
        let mut raw_units = Vec::with_capacity(ranges.len());
        let mut valid_units = true;
        for (ordinal, (unit_id, start, end)) in ranges.iter().enumerate() {
            let converted = usize::try_from(*start).ok().zip(usize::try_from(*end).ok());
            let Some((start, end)) = converted else {
                issues.push(error_issue(
                    "invalid_input_unit",
                    format!("generation run {run_id} has an unrepresentable unit range"),
                    None,
                ));
                valid_units = false;
                continue;
            };
            if unit_id != &format!("u{ordinal:06}")
                || start != expected_start
                || end <= start
                || end > run.raw_text.len()
                || !run.raw_text.is_char_boundary(start)
                || !run.raw_text.is_char_boundary(end)
            {
                issues.push(error_issue(
                    "invalid_input_unit",
                    format!("generation run {run_id} has invalid unit {unit_id}"),
                    None,
                ));
                valid_units = false;
            } else {
                raw_units.push(RawUnit {
                    id: unit_id.clone(),
                    start_byte: start,
                    end_byte: end,
                    text: run.raw_text[start..end].to_owned(),
                });
            }
            expected_start = end;
        }
        if expected_start != run.raw_text.len() {
            issues.push(error_issue(
                "incomplete_input_units",
                format!("generation run {run_id} units do not cover its complete raw input"),
                None,
            ));
            valid_units = false;
        }
        if run.adapter_name != generation::ADAPTER_NAME
            || run.adapter_version != generation::ADAPTER_VERSION
        {
            issues.push(error_issue(
                "unsupported_generation_adapter",
                format!(
                    "generation run {run_id} records unsupported adapter {} version {}",
                    run.adapter_name, run.adapter_version
                ),
                None,
            ));
            valid_units = false;
        } else if valid_units {
            match generation::segment_raw_input(&run.raw_text) {
                Ok(expected_units) if expected_units == raw_units => {}
                Ok(_) => {
                    issues.push(error_issue(
                        "adapter_output_mismatch",
                        format!(
                            "generation run {run_id} units do not match the recorded raw-window adapter"
                        ),
                        None,
                    ));
                    valid_units = false;
                }
                Err(error) => {
                    issues.push(error_issue(
                        "adapter_output_mismatch",
                        format!(
                            "generation run {run_id} input cannot be reproduced by the recorded adapter: {error}"
                        ),
                        None,
                    ));
                    valid_units = false;
                }
            }
        }

        let owned = nodes
            .values()
            .filter(|node| node.generation_run_id == Some(run_id))
            .map(|node| node.id)
            .collect::<BTreeSet<_>>();
        if !owned.contains(&root_id) {
            continue;
        }
        let reachable = descendants(nodes, root_id);
        if owned != reachable {
            issues.push(error_issue(
                "generation_tree_disconnected",
                format!("generation run {run_id} nodes do not form exactly one rooted tree"),
                Some(root_id),
            ));
        }
        if valid_units {
            match generation::parse_and_validate_generated_tree(
                &run.proposal_json,
                &raw_units,
                &run.policy,
            ) {
                Ok(proposal) => {
                    compare_generation(connection, nodes, run_id, root_id, &proposal, issues)?;
                }
                Err(error) => issues.push(error_issue(
                    "invalid_accepted_proposal",
                    format!("generation run {run_id} accepted proposal is invalid: {error}"),
                    Some(root_id),
                )),
            }
        }
    }
    Ok(())
}

fn compare_generation(
    connection: &Connection,
    nodes: &BTreeMap<i64, CanonicalNode>,
    run_id: i64,
    root_id: i64,
    proposal: &GeneratedTree,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), AppError> {
    let stored_ids = generation_preorder(nodes, root_id, run_id);
    if stored_ids.len() != proposal.nodes.len() {
        issues.push(error_issue(
            "generation_proposal_mismatch",
            format!(
                "generation run {run_id} stores {} nodes but its accepted proposal has {}",
                stored_ids.len(),
                proposal.nodes.len()
            ),
            Some(root_id),
        ));
        return Ok(());
    }

    let mut supports = BTreeMap::<i64, BTreeSet<String>>::new();
    let mut statement = connection.prepare(
        "SELECT node_id, unit_id FROM node_support WHERE run_id = ?1 ORDER BY node_id, unit_id",
    )?;
    let rows = statement.query_map([run_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (node_id, unit_id) = row?;
        supports.entry(node_id).or_default().insert(unit_id);
    }

    let mut resolved = BTreeMap::<&str, i64>::new();
    for (generated, stored_id) in proposal.nodes.iter().zip(stored_ids) {
        let Some(stored) = nodes.get(&stored_id) else {
            continue;
        };
        let expected_parent = generated
            .parent_id
            .as_deref()
            .and_then(|parent| resolved.get(parent).copied());
        let expected_support = generated
            .support_unit_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let actual_support = supports.remove(&stored_id).unwrap_or_default();
        if stored.text != generated.text
            || stored.parent_id != expected_parent
            || actual_support != expected_support
        {
            issues.push(error_issue(
                "generation_proposal_mismatch",
                format!(
                    "stored node {stored_id} differs from {} in generation run {run_id}",
                    generated.id
                ),
                Some(stored_id),
            ));
        }
        resolved.insert(&generated.id, stored_id);
    }
    Ok(())
}

fn generation_preorder(
    nodes: &BTreeMap<i64, CanonicalNode>,
    root_id: i64,
    run_id: i64,
) -> Vec<i64> {
    let mut result = Vec::new();
    let mut stack = vec![root_id];
    let mut seen = BTreeSet::new();
    while let Some(node_id) = stack.pop() {
        if !seen.insert(node_id) {
            continue;
        }
        result.push(node_id);
        let mut children = nodes
            .values()
            .filter(|node| {
                node.parent_id == Some(node_id) && node.generation_run_id == Some(run_id)
            })
            .map(|node| (node.position, node.id))
            .collect::<Vec<_>>();
        children.sort_unstable_by(|left, right| right.cmp(left));
        stack.extend(children.into_iter().map(|(_, id)| id));
    }
    result
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn descendants(nodes: &BTreeMap<i64, CanonicalNode>, root_id: i64) -> BTreeSet<i64> {
    let mut result = BTreeSet::from([root_id]);
    loop {
        let before = result.len();
        for node in nodes.values() {
            if node
                .parent_id
                .is_some_and(|parent| result.contains(&parent))
            {
                result.insert(node.id);
            }
        }
        if result.len() == before {
            return result;
        }
    }
}

fn check_index(
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

    let mut statement = connection.prepare(
        "SELECT node_id, text, normalized_text, breadcrumb, normalized_path, \
                content_hash, indexer_version FROM search_units ORDER BY node_id",
    )?;
    let stored = statement
        .query_map([], |row| {
            Ok(index::DerivedUnit {
                node_id: row.get(0)?,
                text: row.get(1)?,
                normalized_text: row.get(2)?,
                breadcrumb: row.get(3)?,
                normalized_path: row.get(4)?,
                content_hash: row.get(5)?,
                indexer_version: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let stored = stored
        .into_iter()
        .map(|unit| (unit.node_id, unit))
        .collect::<BTreeMap<_, _>>();
    for node in nodes.values() {
        let Some(path) = breadcrumb_for(node.id, nodes) else {
            continue;
        };
        let expected = index::derive_unit(node.id, &node.text, &path);
        if stored.get(&node.id) != Some(&expected) {
            issues.push(error_issue(
                "index_unit_mismatch",
                "stored search row differs from canonical node text or path",
                Some(node.id),
            ));
        }
    }
    for node_id in stored.keys() {
        if !nodes.contains_key(node_id) {
            issues.push(error_issue(
                "orphan_index_unit",
                "search row does not belong to a canonical node",
                Some(*node_id),
            ));
        }
    }
    Ok(())
}

fn breadcrumb_for(node_id: i64, nodes: &BTreeMap<i64, CanonicalNode>) -> Option<String> {
    let mut parts = Vec::new();
    let mut current = Some(node_id);
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if !seen.insert(id) {
            return None;
        }
        let node = nodes.get(&id)?;
        parts.push(node.text.as_str());
        current = node.parent_id;
    }
    parts.reverse();
    Some(parts.join(index::BREADCRUMB_SEPARATOR))
}

fn error_issue(code: &str, message: impl Into<String>, node_id: Option<i64>) -> ValidationIssue {
    ValidationIssue {
        severity: ValidationSeverity::Error,
        code: code.to_owned(),
        message: message.into(),
        node_id,
    }
}

#[cfg(test)]
mod tests {
    use super::CanonicalNode;
    use super::breadcrumb_for;
    use std::collections::BTreeMap;

    #[test]
    fn builds_root_to_leaf_breadcrumbs() {
        let nodes = BTreeMap::from([
            (
                1,
                CanonicalNode {
                    id: 1,
                    parent_id: None,
                    generation_run_id: None,
                    text: "Root".to_owned(),
                    position: 0,
                },
            ),
            (
                2,
                CanonicalNode {
                    id: 2,
                    parent_id: Some(1),
                    generation_run_id: None,
                    text: "Leaf".to_owned(),
                    position: 0,
                },
            ),
        ]);
        assert_eq!(breadcrumb_for(2, &nodes).as_deref(), Some("Root / Leaf"));
    }
}
