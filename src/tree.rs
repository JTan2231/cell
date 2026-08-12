use rusqlite::{Connection, OptionalExtension, Transaction, params};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::error::AppError;
use crate::model::Node;
#[cfg(test)]
use crate::model::{GenerationRun, InputUnit, NodeSupport, RawInput};

const POSITION_STEP: i64 = 1024;

#[derive(Clone, Debug)]
pub struct NewNode {
    pub text: String,
    pub position: Option<usize>,
    pub generation_run_id: Option<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct NodeChanges {
    pub text: Option<String>,
}

#[derive(Clone, Debug)]
pub struct NewGenerationRun {
    pub input_id: i64,
    pub adapter_name: String,
    pub adapter_version: String,
    pub model: String,
    pub reasoning_effort: String,
    pub prompt_version: String,
    pub output_schema_version: u32,
    pub node_budget: usize,
    pub max_depth: usize,
    pub max_children: usize,
    pub accepted_proposal_json: String,
}

fn now() -> Result<String, AppError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| AppError::unexpected("timestamp_error", error.to_string()))
}

fn validated_text(text: &str) -> Result<String, AppError> {
    if text.is_empty() || text.trim() != text {
        return Err(AppError::invalid(
            "invalid_node_text",
            "node text must be nonempty and may not have leading or trailing whitespace",
        ));
    }
    Ok(text.to_owned())
}

fn required_text(value: &str, field: &'static str) -> Result<String, AppError> {
    if value.is_empty() || value.trim() != value {
        return Err(AppError::invalid(
            "invalid_generation_metadata",
            format!("{field} must be nonempty and trimmed"),
        ));
    }
    Ok(value.to_owned())
}

fn to_i64(value: usize, field: &'static str) -> Result<i64, AppError> {
    i64::try_from(value).map_err(|_| {
        AppError::invalid(
            "numeric_value_too_large",
            format!("{field} is too large for the library format"),
        )
    })
}

fn to_usize(value: i64, field: &'static str) -> Result<usize, AppError> {
    usize::try_from(value).map_err(|_| {
        AppError::database(
            "invalid_stored_number",
            format!("stored {field} is negative or too large"),
        )
    })
}

fn raw_node(row: &rusqlite::Row<'_>) -> rusqlite::Result<Node> {
    Ok(Node {
        id: row.get(0)?,
        parent_id: row.get(1)?,
        generation_run_id: row.get(2)?,
        text: row.get(3)?,
        position: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

/// Return the revision of the canonical library content.
pub fn library_revision(connection: &Connection) -> Result<i64, AppError> {
    connection
        .query_row(
            "SELECT revision FROM library_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(AppError::from)
}

/// Increment the canonical library revision once for a committed mutation.
pub fn bump_library_revision(transaction: &Transaction<'_>) -> Result<i64, AppError> {
    let revision = library_revision(transaction)?
        .checked_add(1)
        .ok_or_else(|| AppError::database("revision_overflow", "library revision is too large"))?;
    let changed = transaction.execute(
        "UPDATE library_state SET revision = ?1 WHERE singleton = 1",
        [revision],
    )?;
    if changed != 1 {
        return Err(AppError::database(
            "library_state_missing",
            "library revision row is missing",
        ));
    }
    Ok(revision)
}

pub fn get_node(connection: &Connection, node_id: i64) -> Result<Node, AppError> {
    connection
        .query_row(
            "SELECT id, parent_id, generation_run_id, text, position, created_at, updated_at \
             FROM nodes WHERE id = ?1",
            [node_id],
            raw_node,
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found("node_not_found", format!("node {node_id} was not found"))
        })
}

/// Return the display path from the tree root through `node_id`.
pub fn node_path(connection: &Connection, node_id: i64) -> Result<String, AppError> {
    let mut statement = connection.prepare(
        "WITH RECURSIVE ancestors(id, parent_id, text, distance) AS ( \
             SELECT id, parent_id, text, 0 FROM nodes WHERE id = ?1 \
             UNION ALL \
             SELECT n.id, n.parent_id, n.text, ancestors.distance + 1 \
             FROM nodes AS n JOIN ancestors ON ancestors.parent_id = n.id \
         ) \
         SELECT text FROM ancestors ORDER BY distance DESC",
    )?;
    let rows = statement.query_map([node_id], |row| row.get::<_, String>(0))?;
    let parts = rows.collect::<Result<Vec<_>, _>>()?;
    if parts.is_empty() {
        return Err(AppError::not_found(
            "node_not_found",
            format!("node {node_id} was not found"),
        ));
    }
    Ok(parts.join(" / "))
}

fn sibling_ids(
    connection: &Connection,
    parent_id: Option<i64>,
    excluding: Option<i64>,
) -> Result<Vec<i64>, AppError> {
    let mut statement = connection.prepare(
        "SELECT id FROM nodes \
         WHERE parent_id IS ?1 AND (?2 IS NULL OR id <> ?2) \
         ORDER BY position, id",
    )?;
    let rows = statement.query_map(params![parent_id, excluding], |row| row.get(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

fn next_position(connection: &Connection, parent_id: Option<i64>) -> Result<i64, AppError> {
    connection
        .query_row(
            "SELECT COALESCE(MAX(position), -1024) + 1024 \
             FROM nodes WHERE parent_id IS ?1",
            [parent_id],
            |row| row.get(0),
        )
        .map_err(AppError::from)
}

fn next_position_excluding(
    connection: &Connection,
    parent_id: Option<i64>,
    node_id: i64,
) -> Result<i64, AppError> {
    connection
        .query_row(
            "SELECT COALESCE(MAX(position), -1024) + 1024 \
             FROM nodes WHERE parent_id IS ?1 AND id <> ?2",
            params![parent_id, node_id],
            |row| row.get(0),
        )
        .map_err(AppError::from)
}

fn reorder(
    transaction: &Transaction<'_>,
    parent_id: Option<i64>,
    ordered_ids: &[i64],
) -> Result<(), AppError> {
    if ordered_ids.is_empty() {
        return Ok(());
    }
    let max_position = transaction.query_row(
        "SELECT COALESCE(MAX(position), 0) FROM nodes WHERE parent_id IS ?1",
        [parent_id],
        |row| row.get::<_, i64>(0),
    )?;
    let count = i64::try_from(ordered_ids.len()).map_err(|_| {
        AppError::database("too_many_siblings", "too many siblings to assign positions")
    })?;
    let temporary_base = max_position
        .checked_add(POSITION_STEP.saturating_mul(count.saturating_add(2)))
        .ok_or_else(|| {
            AppError::database("position_overflow", "sibling positions are too large")
        })?;
    for (ordinal, node_id) in ordered_ids.iter().enumerate() {
        let ordinal = i64::try_from(ordinal).map_err(|_| {
            AppError::database("too_many_siblings", "too many siblings to assign positions")
        })?;
        transaction.execute(
            "UPDATE nodes SET position = ?1 WHERE id = ?2",
            params![temporary_base + ordinal, node_id],
        )?;
    }
    for (ordinal, node_id) in ordered_ids.iter().enumerate() {
        let ordinal = i64::try_from(ordinal).map_err(|_| {
            AppError::database("too_many_siblings", "too many siblings to assign positions")
        })?;
        transaction.execute(
            "UPDATE nodes SET position = ?1 WHERE id = ?2",
            params![ordinal.saturating_mul(POSITION_STEP), node_id],
        )?;
    }
    Ok(())
}

fn insert_at(
    transaction: &Transaction<'_>,
    parent_id: Option<i64>,
    node_id: i64,
    ordinal: Option<usize>,
) -> Result<(), AppError> {
    let mut ids = sibling_ids(transaction, parent_id, Some(node_id))?;
    let destination = ordinal.unwrap_or(ids.len());
    if destination > ids.len() {
        return Err(AppError::invalid(
            "invalid_position",
            format!(
                "position {destination} is past the end of a {}-item sibling list",
                ids.len()
            ),
        ));
    }
    ids.insert(destination, node_id);
    reorder(transaction, parent_id, &ids)
}

fn insert_root(
    transaction: &Transaction<'_>,
    text: &str,
    generation_run_id: Option<i64>,
) -> Result<i64, AppError> {
    let text = validated_text(text)?;
    let timestamp = now()?;
    let position = next_position(transaction, None)?;
    transaction.execute(
        "INSERT INTO nodes(parent_id, generation_run_id, text, position, created_at, updated_at) \
         VALUES(NULL, ?1, ?2, ?3, ?4, ?4)",
        params![generation_run_id, text, position, timestamp],
    )?;
    Ok(transaction.last_insert_rowid())
}

pub fn create_root(transaction: &Transaction<'_>, text: &str) -> Result<i64, AppError> {
    insert_root(transaction, text, None)
}

pub fn create_generation_root(
    transaction: &Transaction<'_>,
    generation_run_id: i64,
    text: &str,
) -> Result<i64, AppError> {
    insert_root(transaction, text, Some(generation_run_id))
}

pub fn create_root_for_run(
    transaction: &Transaction<'_>,
    text: &str,
    generation_run_id: i64,
) -> Result<i64, AppError> {
    create_generation_root(transaction, generation_run_id, text)
}

pub fn add_node(
    transaction: &Transaction<'_>,
    parent_id: i64,
    new_node: &NewNode,
) -> Result<i64, AppError> {
    let parent = get_node(transaction, parent_id)?;
    if parent.generation_run_id.is_some() && new_node.generation_run_id.is_none() {
        return Err(AppError::conflict(
            "generated_tree_immutable",
            "generated trees cannot be changed with manual node commands",
        ));
    }
    let generation_run_id = new_node.generation_run_id.or(parent.generation_run_id);
    if generation_run_id != parent.generation_run_id {
        return Err(AppError::conflict(
            "generation_run_mismatch",
            "a child must belong to the same generation run as its parent",
        ));
    }
    let text = validated_text(&new_node.text)?;
    if let Some(position) = new_node.position {
        let sibling_count = sibling_ids(transaction, Some(parent_id), None)?.len();
        if position > sibling_count {
            return Err(AppError::invalid(
                "invalid_position",
                format!(
                    "position {position} is past the end of a {sibling_count}-item sibling list"
                ),
            ));
        }
    }
    let timestamp = now()?;
    let position = next_position(transaction, Some(parent_id))?;
    transaction.execute(
        "INSERT INTO nodes(parent_id, generation_run_id, text, position, created_at, updated_at) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?5)",
        params![parent_id, generation_run_id, text, position, timestamp],
    )?;
    let node_id = transaction.last_insert_rowid();
    if new_node.position.is_some() {
        insert_at(transaction, Some(parent_id), node_id, new_node.position)?;
    }
    Ok(node_id)
}

pub fn edit_node(
    transaction: &Transaction<'_>,
    node_id: i64,
    changes: &NodeChanges,
) -> Result<(), AppError> {
    let old = get_node(transaction, node_id)?;
    if old.generation_run_id.is_some() {
        return Err(AppError::conflict(
            "generated_tree_immutable",
            "generated trees cannot be changed with manual node commands",
        ));
    }
    let Some(text) = changes.text.as_deref() else {
        return Err(AppError::invalid(
            "no_changes",
            "node edit requires a new text value",
        ));
    };
    let text = validated_text(text)?;
    transaction.execute(
        "UPDATE nodes SET text = ?1, updated_at = ?2 WHERE id = ?3",
        params![text, now()?, node_id],
    )?;
    Ok(())
}

pub fn subtree_ids(connection: &Connection, node_id: i64) -> Result<Vec<i64>, AppError> {
    get_node(connection, node_id)?;
    let mut statement = connection.prepare(
        "WITH RECURSIVE subtree(id, sort_path) AS ( \
             SELECT id, printf('%020d', position) FROM nodes WHERE id = ?1 \
             UNION ALL \
             SELECT n.id, subtree.sort_path || '/' || printf('%020d', n.position) \
             FROM nodes AS n JOIN subtree ON n.parent_id = subtree.id \
         ) \
         SELECT id FROM subtree ORDER BY sort_path",
    )?;
    let rows = statement.query_map([node_id], |row| row.get(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn subtree_count(connection: &Connection, node_id: i64) -> Result<usize, AppError> {
    Ok(subtree_ids(connection, node_id)?.len())
}

pub fn root_id(connection: &Connection, node_id: i64) -> Result<i64, AppError> {
    get_node(connection, node_id)?;
    connection
        .query_row(
            "WITH RECURSIVE ancestors(id, parent_id) AS ( \
                 SELECT id, parent_id FROM nodes WHERE id = ?1 \
                 UNION ALL \
                 SELECT n.id, n.parent_id FROM nodes AS n \
                 JOIN ancestors AS a ON a.parent_id = n.id \
             ) \
             SELECT id FROM ancestors WHERE parent_id IS NULL LIMIT 1",
            [node_id],
            |row| row.get(0),
        )
        .map_err(AppError::from)
}

fn contains_in_subtree(
    connection: &Connection,
    node_id: i64,
    possible_descendant: i64,
) -> Result<bool, AppError> {
    connection
        .query_row(
            "WITH RECURSIVE subtree(id) AS ( \
                 SELECT id FROM nodes WHERE id = ?1 \
                 UNION ALL \
                 SELECT n.id FROM nodes AS n JOIN subtree AS s ON n.parent_id = s.id \
             ) \
             SELECT EXISTS(SELECT 1 FROM subtree WHERE id = ?2)",
            params![node_id, possible_descendant],
            |row| row.get(0),
        )
        .map_err(AppError::from)
}

pub fn move_node(
    transaction: &Transaction<'_>,
    node_id: i64,
    new_parent_id: i64,
    position: Option<usize>,
) -> Result<(), AppError> {
    let node = get_node(transaction, node_id)?;
    if node.generation_run_id.is_some() {
        return Err(AppError::conflict(
            "generated_tree_immutable",
            "generated trees cannot be changed with manual node commands",
        ));
    }
    let new_parent = get_node(transaction, new_parent_id)?;
    if new_parent.generation_run_id.is_some() {
        return Err(AppError::conflict(
            "generated_tree_immutable",
            "generated trees cannot be changed with manual node commands",
        ));
    }
    let old_parent_id = node.parent_id.ok_or_else(|| {
        AppError::conflict(
            "root_move_not_allowed",
            "root nodes cannot be moved with `node move`",
        )
    })?;
    if contains_in_subtree(transaction, node_id, new_parent_id)? {
        return Err(AppError::conflict(
            "would_create_cycle",
            "the requested parent is inside the node's subtree",
        ));
    }
    if root_id(transaction, node_id)? != root_id(transaction, new_parent_id)? {
        return Err(AppError::conflict(
            "cross_tree_move_not_supported",
            "moving a node between trees is not supported",
        ));
    }

    if position.is_none() {
        let destination_position = if old_parent_id == new_parent_id {
            next_position_excluding(transaction, Some(new_parent_id), node_id)?
        } else {
            next_position(transaction, Some(new_parent_id))?
        };
        transaction.execute(
            "UPDATE nodes SET parent_id = ?1, position = ?2, updated_at = ?3 WHERE id = ?4",
            params![new_parent_id, destination_position, now()?, node_id],
        )?;
        return Ok(());
    }

    if old_parent_id == new_parent_id {
        let mut ids = sibling_ids(transaction, Some(old_parent_id), Some(node_id))?;
        let destination = position.unwrap_or(ids.len());
        if destination > ids.len() {
            return Err(AppError::invalid(
                "invalid_position",
                format!("position {destination} is past the end of the sibling list"),
            ));
        }
        ids.insert(destination, node_id);
        reorder(transaction, Some(old_parent_id), &ids)?;
        transaction.execute(
            "UPDATE nodes SET updated_at = ?1 WHERE id = ?2",
            params![now()?, node_id],
        )?;
    } else {
        let destination_ids = sibling_ids(transaction, Some(new_parent_id), None)?;
        let destination = position.unwrap_or(destination_ids.len());
        if destination > destination_ids.len() {
            return Err(AppError::invalid(
                "invalid_position",
                format!("position {destination} is past the end of the sibling list"),
            ));
        }
        let free_position = next_position(transaction, Some(new_parent_id))?;
        transaction.execute(
            "UPDATE nodes SET parent_id = ?1, position = ?2, updated_at = ?3 WHERE id = ?4",
            params![new_parent_id, free_position, now()?, node_id],
        )?;
        let old_ids = sibling_ids(transaction, Some(old_parent_id), None)?;
        reorder(transaction, Some(old_parent_id), &old_ids)?;
        let mut new_ids = sibling_ids(transaction, Some(new_parent_id), Some(node_id))?;
        new_ids.insert(destination, node_id);
        reorder(transaction, Some(new_parent_id), &new_ids)?;
    }
    Ok(())
}

pub fn delete_node(transaction: &Transaction<'_>, node_id: i64) -> Result<(), AppError> {
    let node = get_node(transaction, node_id)?;
    if node.generation_run_id.is_some() {
        return Err(AppError::conflict(
            "generated_tree_immutable",
            "generated trees cannot be changed with manual node commands",
        ));
    }
    node.parent_id.ok_or_else(|| {
        AppError::conflict(
            "root_delete_not_allowed",
            "root nodes must be deleted with `tree delete`",
        )
    })?;
    transaction.execute("DELETE FROM nodes WHERE id = ?1", [node_id])?;
    Ok(())
}

pub fn delete_tree(transaction: &Transaction<'_>, root_node_id: i64) -> Result<(), AppError> {
    let root = get_node(transaction, root_node_id)?;
    if root.parent_id.is_some() {
        return Err(AppError::not_found(
            "tree_not_found",
            format!("node {root_node_id} is not a tree root"),
        ));
    }
    if let Some(run_id) = root.generation_run_id {
        let input_id = transaction.query_row(
            "SELECT input_id FROM generation_runs WHERE id = ?1",
            [run_id],
            |row| row.get::<_, i64>(0),
        )?;
        transaction.execute("DELETE FROM generation_runs WHERE id = ?1", [run_id])?;
        transaction.execute(
            "DELETE FROM raw_inputs WHERE id = ?1 \
             AND NOT EXISTS(SELECT 1 FROM generation_runs WHERE input_id = ?1)",
            [input_id],
        )?;
    } else {
        transaction.execute("DELETE FROM nodes WHERE id = ?1", [root_node_id])?;
    }
    Ok(())
}

pub fn roots(connection: &Connection) -> Result<Vec<Node>, AppError> {
    nodes_for_query(
        connection,
        "SELECT id, parent_id, generation_run_id, text, position, created_at, updated_at \
         FROM nodes WHERE parent_id IS NULL ORDER BY position, id",
        [],
    )
}

pub fn children(connection: &Connection, node_id: i64) -> Result<Vec<Node>, AppError> {
    get_node(connection, node_id)?;
    nodes_for_query(
        connection,
        "SELECT id, parent_id, generation_run_id, text, position, created_at, updated_at \
         FROM nodes WHERE parent_id = ?1 ORDER BY position, id",
        [node_id],
    )
}

fn nodes_for_query<P: rusqlite::Params>(
    connection: &Connection,
    sql: &str,
    parameters: P,
) -> Result<Vec<Node>, AppError> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(parameters, raw_node)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn subtree_with_depth(
    connection: &Connection,
    root_node_id: i64,
    max_depth: Option<usize>,
) -> Result<Vec<(Node, usize)>, AppError> {
    let root = get_node(connection, root_node_id)?;
    if root.parent_id.is_some() {
        return Err(AppError::not_found(
            "tree_not_found",
            format!("node {root_node_id} is not a tree root"),
        ));
    }
    let depth_limit = max_depth.map_or(i64::MAX, |value| i64::try_from(value).unwrap_or(i64::MAX));
    let mut statement = connection.prepare(
        "WITH RECURSIVE subtree(id, depth, sort_path) AS ( \
             SELECT id, 0, printf('%020d', position) FROM nodes WHERE id = ?1 \
             UNION ALL \
             SELECT n.id, subtree.depth + 1, \
                    subtree.sort_path || '/' || printf('%020d', n.position) \
             FROM nodes AS n JOIN subtree ON n.parent_id = subtree.id \
             WHERE subtree.depth < ?2 \
         ) \
         SELECT n.id, n.parent_id, n.generation_run_id, n.text, n.position, \
                n.created_at, n.updated_at, subtree.depth \
         FROM subtree JOIN nodes AS n ON n.id = subtree.id \
         ORDER BY subtree.sort_path",
    )?;
    let rows = statement.query_map(params![root_node_id, depth_limit], |row| {
        Ok((raw_node(row)?, row.get::<_, i64>(7)?))
    })?;
    rows.map(|row| {
        let (node, depth) = row?;
        Ok((node, to_usize(depth, "tree depth")?))
    })
    .collect()
}

/// Persist one raw input. The caller supplies its lowercase hexadecimal SHA-256.
pub fn insert_raw_input(
    transaction: &Transaction<'_>,
    text: &str,
    sha256: &str,
) -> Result<i64, AppError> {
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(AppError::invalid(
            "invalid_sha256",
            "raw input checksum must be 64 lowercase hexadecimal characters",
        ));
    }
    transaction.execute(
        "INSERT INTO raw_inputs(text, sha256, created_at) VALUES(?1, ?2, ?3)",
        params![text, sha256, now()?],
    )?;
    Ok(transaction.last_insert_rowid())
}

/// Start a generation record. Set its root after inserting the accepted nodes.
pub fn insert_generation_run(
    transaction: &Transaction<'_>,
    run: &NewGenerationRun,
) -> Result<i64, AppError> {
    let adapter_name = required_text(&run.adapter_name, "adapter name")?;
    let adapter_version = required_text(&run.adapter_version, "adapter version")?;
    let model = required_text(&run.model, "model")?;
    let reasoning_effort = required_text(&run.reasoning_effort, "reasoning effort")?;
    let prompt_version = required_text(&run.prompt_version, "prompt version")?;
    if run.output_schema_version == 0
        || run.node_budget == 0
        || run.max_children == 0
        || serde_json::from_str::<serde_json::Value>(&run.accepted_proposal_json).is_err()
    {
        return Err(AppError::invalid(
            "invalid_generation_metadata",
            "generation limits and accepted proposal must be valid",
        ));
    }
    transaction.execute(
        "INSERT INTO generation_runs( \
             input_id, root_node_id, adapter_name, adapter_version, model, reasoning_effort, \
             prompt_version, output_schema_version, node_budget, max_depth, max_children, \
             accepted_proposal_json, created_at \
         ) VALUES(?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            run.input_id,
            adapter_name,
            adapter_version,
            model,
            reasoning_effort,
            prompt_version,
            i64::from(run.output_schema_version),
            to_i64(run.node_budget, "node budget")?,
            to_i64(run.max_depth, "maximum depth")?,
            to_i64(run.max_children, "maximum children")?,
            run.accepted_proposal_json,
            now()?,
        ],
    )?;
    Ok(transaction.last_insert_rowid())
}

pub fn set_generation_root(
    transaction: &Transaction<'_>,
    run_id: i64,
    root_node_id: i64,
) -> Result<(), AppError> {
    let changed = transaction.execute(
        "UPDATE generation_runs SET root_node_id = ?1 \
         WHERE id = ?2 \
           AND root_node_id IS NULL \
           AND EXISTS( \
               SELECT 1 FROM nodes \
               WHERE id = ?1 AND parent_id IS NULL AND generation_run_id = ?2 \
           )",
        params![root_node_id, run_id],
    )?;
    if changed != 1 {
        return Err(AppError::conflict(
            "invalid_generation_root",
            "generation root must be an unset root node from the same run",
        ));
    }
    Ok(())
}

pub fn insert_input_unit(
    transaction: &Transaction<'_>,
    run_id: i64,
    unit_id: &str,
    start_byte: usize,
    end_byte: usize,
) -> Result<(), AppError> {
    if unit_id.is_empty() || unit_id.trim() != unit_id || end_byte <= start_byte {
        return Err(AppError::invalid(
            "invalid_input_unit",
            "input units require a trimmed ID and a nonempty byte range",
        ));
    }
    transaction.execute(
        "INSERT INTO input_units(run_id, unit_id, start_byte, end_byte) \
         VALUES(?1, ?2, ?3, ?4)",
        params![
            run_id,
            unit_id,
            to_i64(start_byte, "unit start byte")?,
            to_i64(end_byte, "unit end byte")?
        ],
    )?;
    Ok(())
}

pub fn insert_node_support(
    transaction: &Transaction<'_>,
    node_id: i64,
    run_id: i64,
    unit_id: &str,
) -> Result<(), AppError> {
    transaction.execute(
        "INSERT INTO node_support(node_id, run_id, unit_id) VALUES(?1, ?2, ?3)",
        params![node_id, run_id, unit_id],
    )?;
    Ok(())
}

#[cfg(test)]
fn get_raw_input(connection: &Connection, input_id: i64) -> Result<RawInput, AppError> {
    connection
        .query_row(
            "SELECT id, text, sha256, created_at FROM raw_inputs WHERE id = ?1",
            [input_id],
            |row| {
                Ok(RawInput {
                    id: row.get(0)?,
                    text: row.get(1)?,
                    sha256: row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found(
                "raw_input_not_found",
                format!("raw input {input_id} was not found"),
            )
        })
}

#[cfg(test)]
fn get_generation_run(connection: &Connection, run_id: i64) -> Result<GenerationRun, AppError> {
    let raw = connection
        .query_row(
            "SELECT id, input_id, root_node_id, adapter_name, adapter_version, model, \
                    reasoning_effort, prompt_version, output_schema_version, node_budget, \
                    max_depth, max_children, accepted_proposal_json, created_at \
             FROM generation_runs WHERE id = ?1",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, i64>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, String>(13)?,
                ))
            },
        )
        .optional()?;
    let Some(raw) = raw else {
        return Err(AppError::not_found(
            "generation_run_not_found",
            format!("generation run {run_id} was not found"),
        ));
    };
    Ok(GenerationRun {
        id: raw.0,
        input_id: raw.1,
        root_node_id: raw.2,
        adapter_name: raw.3,
        adapter_version: raw.4,
        model: raw.5,
        reasoning_effort: raw.6,
        prompt_version: raw.7,
        output_schema_version: u32::try_from(raw.8).map_err(|_| {
            AppError::database("invalid_stored_number", "invalid output schema version")
        })?,
        node_budget: to_usize(raw.9, "node budget")?,
        max_depth: to_usize(raw.10, "maximum depth")?,
        max_children: to_usize(raw.11, "maximum children")?,
        accepted_proposal_json: raw.12,
        created_at: raw.13,
    })
}

#[cfg(test)]
fn input_units(connection: &Connection, run_id: i64) -> Result<Vec<InputUnit>, AppError> {
    let mut statement = connection.prepare(
        "SELECT run_id, unit_id, start_byte, end_byte \
         FROM input_units WHERE run_id = ?1 ORDER BY start_byte, unit_id",
    )?;
    let rows = statement.query_map([run_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    rows.map(|row| {
        let (run_id, unit_id, start_byte, end_byte) = row?;
        Ok(InputUnit {
            run_id,
            unit_id,
            start_byte: to_usize(start_byte, "unit start byte")?,
            end_byte: to_usize(end_byte, "unit end byte")?,
        })
    })
    .collect()
}

#[cfg(test)]
fn node_support(connection: &Connection, node_id: i64) -> Result<Vec<NodeSupport>, AppError> {
    get_node(connection, node_id)?;
    let mut statement = connection.prepare(
        "SELECT node_id, run_id, unit_id FROM node_support \
         WHERE node_id = ?1 ORDER BY unit_id",
    )?;
    let rows = statement.query_map([node_id], |row| {
        Ok(NodeSupport {
            node_id: row.get(0)?,
            run_id: row.get(1)?,
            unit_id: row.get(2)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use rusqlite::Connection;
    use tempfile::TempDir;

    use super::*;

    type TestResult = Result<(), Box<dyn Error>>;

    fn library() -> Result<(TempDir, Connection), Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let connection = crate::db::init(&directory.path().join("annals.db"))?;
        Ok((directory, connection))
    }

    fn child(text: &str, position: Option<usize>) -> NewNode {
        NewNode {
            text: text.to_owned(),
            position,
            generation_run_id: None,
        }
    }

    fn node_ids(nodes: &[Node]) -> Vec<i64> {
        nodes.iter().map(|node| node.id).collect()
    }

    #[test]
    fn homogeneous_tree_crud_preserves_order_and_paths() -> TestResult {
        let (_directory, mut connection) = library()?;
        let transaction = connection.transaction()?;
        let root = create_root(&transaction, "Root")?;
        let alpha = add_node(&transaction, root, &child("Alpha", None))?;
        let gamma = add_node(&transaction, root, &child("Gamma", None))?;
        let beta = add_node(&transaction, root, &child("Beta", Some(1)))?;
        assert_eq!(node_path(&transaction, beta)?, "Root / Beta");
        edit_node(
            &transaction,
            beta,
            &NodeChanges {
                text: Some("Beta revised".to_owned()),
            },
        )?;
        transaction.commit()?;

        assert_eq!(
            node_ids(&children(&connection, root)?),
            [alpha, beta, gamma]
        );
        assert_eq!(get_node(&connection, beta)?.text, "Beta revised");
        Ok(())
    }

    #[test]
    fn generated_record_round_trips_with_units_and_support() -> TestResult {
        let (_directory, mut connection) = library()?;
        let transaction = connection.transaction()?;
        let input_id = insert_raw_input(&transaction, "alpha beta", &"a".repeat(64))?;
        let run_id = insert_generation_run(
            &transaction,
            &NewGenerationRun {
                input_id,
                adapter_name: "raw-window".to_owned(),
                adapter_version: "1".to_owned(),
                model: "gpt-5.6-terra".to_owned(),
                reasoning_effort: "medium".to_owned(),
                prompt_version: "1".to_owned(),
                output_schema_version: 1,
                node_budget: 32,
                max_depth: 6,
                max_children: 6,
                accepted_proposal_json: r#"{"schema_version":1,"nodes":[]}"#.to_owned(),
            },
        )?;
        insert_input_unit(&transaction, run_id, "u000000", 0, 10)?;
        let root = create_generation_root(&transaction, run_id, "Family")?;
        let leaf = add_node(
            &transaction,
            root,
            &NewNode {
                text: "Alpha".to_owned(),
                position: None,
                generation_run_id: Some(run_id),
            },
        )?;
        insert_node_support(&transaction, leaf, run_id, "u000000")?;
        set_generation_root(&transaction, run_id, root)?;
        transaction.commit()?;

        let run = get_generation_run(&connection, run_id)?;
        assert_eq!(run.root_node_id, Some(root));
        assert_eq!(get_raw_input(&connection, input_id)?.text, "alpha beta");
        assert_eq!(input_units(&connection, run_id)?[0].start_byte, 0);
        assert_eq!(node_support(&connection, leaf)?[0].unit_id, "u000000");
        Ok(())
    }

    #[test]
    fn moves_reject_cycles_and_cross_tree_changes() -> TestResult {
        let (_directory, mut connection) = library()?;
        let transaction = connection.transaction()?;
        let first_root = create_root(&transaction, "First")?;
        let branch = add_node(&transaction, first_root, &child("Branch", None))?;
        let descendant = add_node(&transaction, branch, &child("Descendant", None))?;
        let second_root = create_root(&transaction, "Second")?;

        let cycle = move_node(&transaction, branch, descendant, None);
        assert!(cycle.is_err_and(|error| error.code() == "would_create_cycle"));
        let cross_tree = move_node(&transaction, branch, second_root, None);
        assert!(cross_tree.is_err_and(|error| error.code() == "cross_tree_move_not_supported"));
        transaction.commit()?;
        Ok(())
    }
}
