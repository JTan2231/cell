use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::error::AppError;
use crate::model::{Node, NodeKind, Source};

const POSITION_STEP: i64 = 1024;

#[derive(Clone, Debug, Default)]
pub struct SourceFields {
    pub locator: Option<String>,
    pub media_type: Option<String>,
    pub checksum: Option<String>,
    pub captured_at: Option<String>,
}

impl SourceFields {
    #[must_use]
    pub fn supplied(&self) -> bool {
        self.locator.is_some()
            || self.media_type.is_some()
            || self.checksum.is_some()
            || self.captured_at.is_some()
    }
}

#[derive(Clone, Debug)]
pub struct NewNode {
    pub kind: NodeKind,
    pub title: String,
    pub body: String,
    pub position: Option<usize>,
    pub source: SourceFields,
}

#[derive(Clone, Debug, Default)]
pub struct NodeChanges {
    pub kind: Option<NodeKind>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub source: SourceFields,
}

#[derive(Clone, Copy, Debug)]
pub struct EditOutcome {
    pub title_changed: bool,
}

#[derive(Clone, Debug)]
struct RawNode {
    id: i64,
    parent_id: Option<i64>,
    kind: String,
    title: String,
    body: String,
    position: i64,
    created_at: String,
    updated_at: String,
    source_node_id: Option<i64>,
    locator: Option<String>,
    media_type: Option<String>,
    checksum: Option<String>,
    captured_at: Option<String>,
}

fn now() -> Result<String, AppError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| AppError::unexpected("timestamp_error", error.to_string()))
}

fn validated_title(title: &str) -> Result<String, AppError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::invalid(
            "invalid_title",
            "a node title cannot be empty",
        ));
    }
    Ok(title.to_owned())
}

fn validate_captured_at(value: Option<&str>) -> Result<(), AppError> {
    if let Some(value) = value {
        OffsetDateTime::parse(value, &Rfc3339).map_err(|_| {
            AppError::invalid(
                "invalid_captured_at",
                "--captured-at must be an RFC 3339 timestamp",
            )
        })?;
    }
    Ok(())
}

fn validate_checksum(value: Option<&str>) -> Result<(), AppError> {
    let Some(value) = value else {
        return Ok(());
    };
    let Some((algorithm, digest)) = value.split_once(':') else {
        return Err(AppError::invalid(
            "invalid_checksum",
            "--checksum must use an algorithm-prefixed value such as sha256:...",
        ));
    };
    let valid_algorithm = !algorithm.is_empty()
        && algorithm
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if !valid_algorithm || digest.trim().is_empty() {
        return Err(AppError::invalid(
            "invalid_checksum",
            "--checksum must use an algorithm-prefixed value such as sha256:...",
        ));
    }
    Ok(())
}

fn raw_node(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawNode> {
    Ok(RawNode {
        id: row.get(0)?,
        parent_id: row.get(1)?,
        kind: row.get(2)?,
        title: row.get(3)?,
        body: row.get(4)?,
        position: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        source_node_id: row.get(8)?,
        locator: row.get(9)?,
        media_type: row.get(10)?,
        checksum: row.get(11)?,
        captured_at: row.get(12)?,
    })
}

fn materialize(raw: RawNode) -> Result<Node, AppError> {
    let kind = NodeKind::from_str(&raw.kind).map_err(|_| {
        AppError::database(
            "invalid_node_kind",
            format!("node {} has invalid kind {:?}", raw.id, raw.kind),
        )
    })?;
    let source = raw.source_node_id.map(|node_id| Source {
        node_id,
        locator: raw.locator,
        media_type: raw.media_type,
        checksum: raw.checksum,
        captured_at: raw.captured_at,
    });
    Ok(Node {
        id: raw.id,
        parent_id: raw.parent_id,
        kind,
        title: raw.title,
        body: raw.body,
        position: raw.position,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
        source,
    })
}

pub fn get_node(connection: &Connection, node_id: i64) -> Result<Node, AppError> {
    let raw = connection
        .query_row(
            "SELECT n.id, n.parent_id, n.kind, n.title, n.body, n.position, \
                    n.created_at, n.updated_at, s.node_id, s.locator, s.media_type, \
                    s.checksum, s.captured_at \
             FROM nodes AS n LEFT JOIN sources AS s ON s.node_id = n.id \
             WHERE n.id = ?1",
            [node_id],
            raw_node,
        )
        .optional()?;
    raw.map_or_else(
        || {
            Err(AppError::not_found(
                "node_not_found",
                format!("node {node_id} was not found"),
            ))
        },
        materialize,
    )
}

/// Return the display path from the tree root through `node_id`.
pub fn node_path(connection: &Connection, node_id: i64) -> Result<String, AppError> {
    let mut statement = connection.prepare(
        "WITH RECURSIVE ancestors(id, parent_id, title, distance) AS ( \
             SELECT id, parent_id, title, 0 FROM nodes WHERE id = ?1 \
             UNION ALL \
             SELECT n.id, n.parent_id, n.title, ancestors.distance + 1 \
             FROM nodes AS n JOIN ancestors ON ancestors.parent_id = n.id \
         ) \
         SELECT title FROM ancestors ORDER BY distance DESC",
    )?;
    let rows = statement.query_map([node_id], |row| row.get::<_, String>(0))?;
    let titles = rows.collect::<Result<Vec<_>, _>>()?;
    if titles.is_empty() {
        return Err(AppError::not_found(
            "node_not_found",
            format!("node {node_id} was not found"),
        ));
    }
    Ok(titles.join(" / "))
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

pub fn create_root(
    transaction: &Transaction<'_>,
    title: &str,
    body: &str,
) -> Result<i64, AppError> {
    let title = validated_title(title)?;
    let timestamp = now()?;
    let position = next_position(transaction, None)?;
    transaction.execute(
        "INSERT INTO nodes(parent_id, kind, title, body, position, created_at, updated_at) \
         VALUES(NULL, 'topic', ?1, ?2, ?3, ?4, ?4)",
        params![title, body, position, timestamp],
    )?;
    Ok(transaction.last_insert_rowid())
}

pub fn add_node(
    transaction: &Transaction<'_>,
    parent_id: i64,
    new_node: &NewNode,
) -> Result<i64, AppError> {
    let parent = get_node(transaction, parent_id)?;
    if parent.kind == NodeKind::Source {
        return Err(AppError::conflict(
            "source_cannot_have_children",
            format!("source node {parent_id} cannot have children"),
        ));
    }
    if new_node.kind == NodeKind::Topic && new_node.source.supplied() {
        return Err(AppError::invalid(
            "source_metadata_for_topic",
            "source metadata can only be supplied for a source node",
        ));
    }
    validate_captured_at(new_node.source.captured_at.as_deref())?;
    validate_checksum(new_node.source.checksum.as_deref())?;
    let title = validated_title(&new_node.title)?;
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
        "INSERT INTO nodes(parent_id, kind, title, body, position, created_at, updated_at) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![
            parent_id,
            new_node.kind.as_str(),
            title,
            new_node.body,
            position,
            timestamp
        ],
    )?;
    let node_id = transaction.last_insert_rowid();
    if new_node.kind == NodeKind::Source {
        transaction.execute(
            "INSERT INTO sources(node_id, locator, media_type, checksum, captured_at) \
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                node_id,
                new_node.source.locator,
                new_node.source.media_type,
                new_node.source.checksum,
                new_node.source.captured_at
            ],
        )?;
    }
    if new_node.position.is_some() {
        insert_at(transaction, Some(parent_id), node_id, new_node.position)?;
    }
    Ok(node_id)
}

pub fn child_count(connection: &Connection, node_id: i64) -> Result<usize, AppError> {
    let count = connection.query_row(
        "SELECT COUNT(*) FROM nodes WHERE parent_id = ?1",
        [node_id],
        |row| row.get::<_, i64>(0),
    )?;
    usize::try_from(count)
        .map_err(|_| AppError::database("invalid_count", "database returned a negative count"))
}

pub fn edit_node(
    transaction: &Transaction<'_>,
    node_id: i64,
    changes: &NodeChanges,
) -> Result<EditOutcome, AppError> {
    let old = get_node(transaction, node_id)?;
    if changes.kind.is_none()
        && changes.title.is_none()
        && changes.body.is_none()
        && !changes.source.supplied()
    {
        return Err(AppError::invalid(
            "no_changes",
            "node edit requires at least one change",
        ));
    }
    let kind = changes.kind.unwrap_or(old.kind);
    let title = match changes.title.as_deref() {
        Some(value) => validated_title(value)?,
        None => old.title.clone(),
    };
    if kind == NodeKind::Source && child_count(transaction, node_id)? > 0 {
        return Err(AppError::conflict(
            "source_cannot_have_children",
            format!("node {node_id} has children and cannot become a source"),
        ));
    }
    if kind == NodeKind::Topic && changes.source.supplied() {
        return Err(AppError::invalid(
            "source_metadata_for_topic",
            "source metadata can only be supplied for a source node",
        ));
    }
    validate_captured_at(changes.source.captured_at.as_deref())?;
    validate_checksum(changes.source.checksum.as_deref())?;
    let body = changes.body.as_deref().unwrap_or(&old.body);
    let timestamp = now()?;
    transaction.execute(
        "UPDATE nodes SET kind = ?1, title = ?2, body = ?3, updated_at = ?4 WHERE id = ?5",
        params![kind.as_str(), title, body, timestamp, node_id],
    )?;

    match (old.kind, kind) {
        (NodeKind::Topic, NodeKind::Source) => {
            transaction.execute(
                "INSERT INTO sources(node_id, locator, media_type, checksum, captured_at) \
                 VALUES(?1, ?2, ?3, ?4, ?5)",
                params![
                    node_id,
                    changes.source.locator,
                    changes.source.media_type,
                    changes.source.checksum,
                    changes.source.captured_at
                ],
            )?;
        }
        (NodeKind::Source, NodeKind::Topic) => {
            transaction.execute("DELETE FROM sources WHERE node_id = ?1", [node_id])?;
        }
        (NodeKind::Source, NodeKind::Source) if changes.source.supplied() => {
            transaction.execute(
                "UPDATE sources SET \
                    locator = COALESCE(?1, locator), \
                    media_type = COALESCE(?2, media_type), \
                    checksum = COALESCE(?3, checksum), \
                    captured_at = COALESCE(?4, captured_at) \
                 WHERE node_id = ?5",
                params![
                    changes.source.locator,
                    changes.source.media_type,
                    changes.source.checksum,
                    changes.source.captured_at,
                    node_id
                ],
            )?;
        }
        _ => {}
    }
    Ok(EditOutcome {
        title_changed: title != old.title,
    })
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
    let new_parent = get_node(transaction, new_parent_id)?;
    let old_parent_id = node.parent_id.ok_or_else(|| {
        AppError::conflict(
            "root_move_not_allowed",
            "root nodes cannot be moved with `node move`",
        )
    })?;
    if new_parent.kind == NodeKind::Source {
        return Err(AppError::conflict(
            "source_cannot_have_children",
            format!("source node {new_parent_id} cannot have children"),
        ));
    }
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
    transaction.execute("DELETE FROM nodes WHERE id = ?1", [root_node_id])?;
    Ok(())
}

pub fn roots(connection: &Connection) -> Result<Vec<Node>, AppError> {
    nodes_for_query(
        connection,
        "SELECT n.id, n.parent_id, n.kind, n.title, n.body, n.position, \
                n.created_at, n.updated_at, s.node_id, s.locator, s.media_type, \
                s.checksum, s.captured_at \
         FROM nodes AS n LEFT JOIN sources AS s ON s.node_id = n.id \
         WHERE n.parent_id IS NULL ORDER BY n.position, n.id",
        [],
    )
}

pub fn children(connection: &Connection, node_id: i64) -> Result<Vec<Node>, AppError> {
    get_node(connection, node_id)?;
    nodes_for_query(
        connection,
        "SELECT n.id, n.parent_id, n.kind, n.title, n.body, n.position, \
                n.created_at, n.updated_at, s.node_id, s.locator, s.media_type, \
                s.checksum, s.captured_at \
         FROM nodes AS n LEFT JOIN sources AS s ON s.node_id = n.id \
         WHERE n.parent_id = ?1 ORDER BY n.position, n.id",
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
    rows.map(|row| row.map_err(AppError::from).and_then(materialize))
        .collect()
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
         SELECT n.id, n.parent_id, n.kind, n.title, n.body, n.position, \
                n.created_at, n.updated_at, s.node_id, s.locator, s.media_type, \
                s.checksum, s.captured_at, subtree.depth \
         FROM subtree JOIN nodes AS n ON n.id = subtree.id \
         LEFT JOIN sources AS s ON s.node_id = n.id \
         ORDER BY subtree.sort_path",
    )?;
    let rows = statement.query_map(params![root_node_id, depth_limit], |row| {
        Ok((raw_node(row)?, row.get::<_, i64>(13)?))
    })?;
    rows.map(|row| {
        let (raw, depth) = row?;
        let depth = usize::try_from(depth).map_err(|_| {
            AppError::database("invalid_depth", "database returned a negative tree depth")
        })?;
        Ok((materialize(raw)?, depth))
    })
    .collect()
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

    fn topic(title: &str, position: Option<usize>) -> NewNode {
        NewNode {
            kind: NodeKind::Topic,
            title: title.to_owned(),
            body: String::new(),
            position,
            source: SourceFields::default(),
        }
    }

    fn source(title: &str) -> NewNode {
        NewNode {
            kind: NodeKind::Source,
            title: title.to_owned(),
            body: format!("source body for {title}"),
            position: None,
            source: SourceFields::default(),
        }
    }

    fn node_ids(nodes: &[Node]) -> Vec<i64> {
        nodes.iter().map(|node| node.id).collect()
    }

    fn positions(nodes: &[Node]) -> Vec<i64> {
        nodes.iter().map(|node| node.position).collect()
    }

    #[test]
    fn inserts_at_requested_ordinals_and_rejects_out_of_range_positions() -> TestResult {
        let (_directory, mut connection) = library()?;
        let transaction = connection.transaction()?;
        let root = create_root(&transaction, "Root", "")?;
        let alpha = add_node(&transaction, root, &topic("Alpha", None))?;
        let gamma = add_node(&transaction, root, &topic("Gamma", None))?;
        let beta = add_node(&transaction, root, &topic("Beta", Some(1)))?;

        let Err(error) = add_node(&transaction, root, &topic("Too far", Some(4))) else {
            return Err("an out-of-range insertion unexpectedly succeeded".into());
        };
        assert_eq!(error.code(), "invalid_position");
        transaction.commit()?;

        let children = children(&connection, root)?;
        assert_eq!(node_ids(&children), [alpha, beta, gamma]);
        assert_eq!(positions(&children), [0, POSITION_STEP, 2 * POSITION_STEP]);
        Ok(())
    }

    #[test]
    fn node_path_is_root_to_node_and_reports_missing_nodes() -> TestResult {
        let (_directory, mut connection) = library()?;
        let transaction = connection.transaction()?;
        let root = create_root(&transaction, "Root", "")?;
        let branch = add_node(&transaction, root, &topic("Branch", None))?;
        let leaf = add_node(&transaction, branch, &topic("Leaf", None))?;

        assert_eq!(node_path(&transaction, leaf)?, "Root / Branch / Leaf");
        let Err(error) = node_path(&transaction, i64::MAX) else {
            return Err("a missing node unexpectedly produced a path".into());
        };
        assert_eq!(error.code(), "node_not_found");
        transaction.commit()?;
        Ok(())
    }

    #[test]
    fn sources_reject_children_and_topics_reject_source_metadata() -> TestResult {
        let (_directory, mut connection) = library()?;
        let transaction = connection.transaction()?;
        let root = create_root(&transaction, "Root", "")?;
        let source_id = add_node(&transaction, root, &source("Source"))?;

        let Err(error) = add_node(&transaction, source_id, &topic("Child", None)) else {
            return Err("a child was unexpectedly added beneath a source".into());
        };
        assert_eq!(error.code(), "source_cannot_have_children");

        let topic_with_source_metadata = NewNode {
            source: SourceFields {
                locator: Some("https://example.test".to_owned()),
                ..SourceFields::default()
            },
            ..topic("Topic", None)
        };
        let Err(error) = add_node(&transaction, root, &topic_with_source_metadata) else {
            return Err("source metadata was unexpectedly accepted for a topic".into());
        };
        assert_eq!(error.code(), "source_metadata_for_topic");

        let invalid_checksum = NewNode {
            source: SourceFields {
                checksum: Some("unversioned-digest".to_owned()),
                ..SourceFields::default()
            },
            ..source("Invalid checksum")
        };
        let Err(error) = add_node(&transaction, root, &invalid_checksum) else {
            return Err("an unversioned checksum was unexpectedly accepted".into());
        };
        assert_eq!(error.code(), "invalid_checksum");
        transaction.commit()?;

        assert_eq!(child_count(&connection, source_id)?, 0);
        assert_eq!(node_ids(&children(&connection, root)?), [source_id]);
        Ok(())
    }

    #[test]
    fn kind_changes_maintain_source_rows_and_preserve_parent_rules() -> TestResult {
        let (_directory, mut connection) = library()?;
        let transaction = connection.transaction()?;
        let root = create_root(&transaction, "Root", "")?;
        let leaf = add_node(&transaction, root, &topic("Leaf", None))?;
        let parent = add_node(&transaction, root, &topic("Parent", None))?;
        let _child = add_node(&transaction, parent, &topic("Child", None))?;

        let Err(error) = edit_node(&transaction, leaf, &NodeChanges::default()) else {
            return Err("an empty edit unexpectedly succeeded".into());
        };
        assert_eq!(error.code(), "no_changes");

        let to_source = NodeChanges {
            kind: Some(NodeKind::Source),
            source: SourceFields {
                locator: Some("paper:42".to_owned()),
                captured_at: Some("2026-08-11T12:00:00Z".to_owned()),
                ..SourceFields::default()
            },
            ..NodeChanges::default()
        };
        edit_node(&transaction, leaf, &to_source)?;
        let converted = get_node(&transaction, leaf)?;
        assert_eq!(converted.kind, NodeKind::Source);
        assert_eq!(
            converted
                .source
                .as_ref()
                .and_then(|item| item.locator.as_deref()),
            Some("paper:42")
        );

        edit_node(
            &transaction,
            leaf,
            &NodeChanges {
                kind: Some(NodeKind::Topic),
                ..NodeChanges::default()
            },
        )?;
        let converted_back = get_node(&transaction, leaf)?;
        assert_eq!(converted_back.kind, NodeKind::Topic);
        assert!(converted_back.source.is_none());

        let Err(error) = edit_node(&transaction, parent, &to_source) else {
            return Err("a topic with children unexpectedly became a source".into());
        };
        assert_eq!(error.code(), "source_cannot_have_children");
        transaction.commit()?;

        assert_eq!(get_node(&connection, parent)?.kind, NodeKind::Topic);
        Ok(())
    }

    #[test]
    fn move_rejects_cycles_cross_tree_moves_roots_and_source_parents() -> TestResult {
        let (_directory, mut connection) = library()?;
        let transaction = connection.transaction()?;
        let first_root = create_root(&transaction, "First", "")?;
        let branch = add_node(&transaction, first_root, &topic("Branch", None))?;
        let descendant = add_node(&transaction, branch, &topic("Descendant", None))?;
        let source_id = add_node(&transaction, first_root, &source("Source"))?;
        let second_root = create_root(&transaction, "Second", "")?;
        let other_tree_node = add_node(&transaction, second_root, &topic("Other", None))?;

        let Err(error) = move_node(&transaction, branch, descendant, None) else {
            return Err("a cycle-producing move unexpectedly succeeded".into());
        };
        assert_eq!(error.code(), "would_create_cycle");

        let Err(error) = move_node(&transaction, branch, other_tree_node, None) else {
            return Err("a cross-tree move unexpectedly succeeded".into());
        };
        assert_eq!(error.code(), "cross_tree_move_not_supported");

        let Err(error) = move_node(&transaction, first_root, branch, None) else {
            return Err("a root move unexpectedly succeeded".into());
        };
        assert_eq!(error.code(), "root_move_not_allowed");

        let Err(error) = move_node(&transaction, branch, source_id, None) else {
            return Err("a move beneath a source unexpectedly succeeded".into());
        };
        assert_eq!(error.code(), "source_cannot_have_children");
        transaction.commit()?;

        assert_eq!(get_node(&connection, branch)?.parent_id, Some(first_root));
        assert_eq!(get_node(&connection, descendant)?.parent_id, Some(branch));
        Ok(())
    }

    #[test]
    fn ordinary_moves_and_deletes_preserve_spaced_positions() -> TestResult {
        let (_directory, mut connection) = library()?;
        let transaction = connection.transaction()?;
        let root = create_root(&transaction, "Root", "")?;
        let left = add_node(&transaction, root, &topic("Left", None))?;
        let right = add_node(&transaction, root, &topic("Right", None))?;
        let left_first = add_node(&transaction, left, &topic("Left first", None))?;
        let left_second = add_node(&transaction, left, &topic("Left second", None))?;
        let left_third = add_node(&transaction, left, &topic("Left third", None))?;
        let right_first = add_node(&transaction, right, &topic("Right first", None))?;

        let changes_before = transaction.total_changes();
        move_node(&transaction, left_first, left, None)?;
        assert_eq!(transaction.total_changes() - changes_before, 1);
        let left_children = children(&transaction, left)?;
        assert_eq!(
            node_ids(&left_children),
            [left_second, left_third, left_first]
        );
        assert_eq!(
            positions(&left_children),
            [POSITION_STEP, 2 * POSITION_STEP, 3 * POSITION_STEP]
        );

        let changes_before = transaction.total_changes();
        move_node(&transaction, left_second, right, None)?;
        assert_eq!(transaction.total_changes() - changes_before, 1);
        let right_children = children(&transaction, right)?;
        assert_eq!(node_ids(&right_children), [right_first, left_second]);
        assert_eq!(positions(&right_children), [0, POSITION_STEP]);
        let left_children = children(&transaction, left)?;
        assert_eq!(node_ids(&left_children), [left_third, left_first]);
        assert_eq!(
            positions(&left_children),
            [2 * POSITION_STEP, 3 * POSITION_STEP]
        );

        let changes_before = transaction.total_changes();
        delete_node(&transaction, left_third)?;
        assert_eq!(transaction.total_changes() - changes_before, 1);
        let left_children = children(&transaction, left)?;
        assert_eq!(node_ids(&left_children), [left_first]);
        assert_eq!(positions(&left_children), [3 * POSITION_STEP]);
        transaction.commit()?;
        Ok(())
    }

    #[test]
    fn moves_reorder_both_parent_lists() -> TestResult {
        let (_directory, mut connection) = library()?;
        let transaction = connection.transaction()?;
        let root = create_root(&transaction, "Root", "")?;
        let left = add_node(&transaction, root, &topic("Left", None))?;
        let right = add_node(&transaction, root, &topic("Right", None))?;
        let left_first = add_node(&transaction, left, &topic("Left first", None))?;
        let left_second = add_node(&transaction, left, &topic("Left second", None))?;
        let right_first = add_node(&transaction, right, &topic("Right first", None))?;

        move_node(&transaction, left_first, right, Some(0))?;
        move_node(&transaction, right_first, right, Some(0))?;

        let Err(error) = move_node(&transaction, left_second, right, Some(4)) else {
            return Err("an out-of-range move unexpectedly succeeded".into());
        };
        assert_eq!(error.code(), "invalid_position");
        transaction.commit()?;

        let left_children = children(&connection, left)?;
        assert_eq!(node_ids(&left_children), [left_second]);
        assert_eq!(positions(&left_children), [0]);
        let right_children = children(&connection, right)?;
        assert_eq!(node_ids(&right_children), [right_first, left_first]);
        assert_eq!(positions(&right_children), [0, POSITION_STEP]);
        assert_eq!(root_id(&connection, left_first)?, root);
        Ok(())
    }

    #[test]
    fn deletion_cascades_subtrees_and_protects_roots() -> TestResult {
        let (_directory, mut connection) = library()?;
        let transaction = connection.transaction()?;
        let first_root = create_root(&transaction, "First", "")?;
        let second_root = create_root(&transaction, "Second", "")?;
        let branch = add_node(&transaction, first_root, &topic("Branch", None))?;
        let source_id = add_node(&transaction, branch, &source("Source"))?;
        let leaf = add_node(&transaction, first_root, &topic("Leaf", None))?;

        let Err(error) = delete_node(&transaction, first_root) else {
            return Err("a root was unexpectedly deleted through node delete".into());
        };
        assert_eq!(error.code(), "root_delete_not_allowed");

        let Err(error) = delete_tree(&transaction, leaf) else {
            return Err("a non-root node was unexpectedly deleted as a tree".into());
        };
        assert_eq!(error.code(), "tree_not_found");

        delete_node(&transaction, branch)?;
        let Err(error) = get_node(&transaction, source_id) else {
            return Err("a descendant unexpectedly survived subtree deletion".into());
        };
        assert_eq!(error.code(), "node_not_found");
        let remaining_children = children(&transaction, first_root)?;
        assert_eq!(node_ids(&remaining_children), [leaf]);
        assert_eq!(positions(&remaining_children), [POSITION_STEP]);

        delete_tree(&transaction, first_root)?;
        transaction.commit()?;

        let roots = roots(&connection)?;
        assert_eq!(node_ids(&roots), [second_root]);
        assert_eq!(positions(&roots), [POSITION_STEP]);
        assert_eq!(subtree_count(&connection, second_root)?, 1);
        Ok(())
    }
}
