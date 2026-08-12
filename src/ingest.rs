use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Deserializer, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::error::AppError;
use crate::index;
use crate::model::{Node, NodeKind};
use crate::tree;

const POSITION_STEP: i64 = 1024;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(untagged)]
enum NodeRef {
    Existing(i64),
    Created(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceContent {
    #[serde(deserialize_with = "required_nullable")]
    locator: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    media_type: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    checksum: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    captured_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeContent {
    kind: NodeKind,
    title: String,
    body: String,
    #[serde(deserialize_with = "required_nullable")]
    source: Option<SourceContent>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateNode {
    #[serde(rename = "ref")]
    reference: String,
    node: NodeContent,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaceNode {
    id: i64,
    node: NodeContent,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteSubtree {
    root_id: i64,
    expected_node_ids: Vec<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChildOrder {
    parent: NodeRef,
    children: Vec<NodeRef>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IngestPlan {
    tree_root_id: i64,
    base_revision: i64,
    create_nodes: Vec<CreateNode>,
    replace_nodes: Vec<ReplaceNode>,
    delete_subtrees: Vec<DeleteSubtree>,
    child_orders: Vec<ChildOrder>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MoveReceipt {
    pub node_id: i64,
    pub from_parent: i64,
    pub to_parent: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResolvedChildOrder {
    pub parent: i64,
    pub children: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IngestOutput {
    pub previous_revision: i64,
    pub new_revision: i64,
    pub created: BTreeMap<String, i64>,
    pub replaced_node_ids: Vec<i64>,
    pub moved: Vec<MoveReceipt>,
    pub deleted_node_ids: Vec<i64>,
    pub final_child_orders: Vec<ResolvedChildOrder>,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

#[derive(Debug)]
struct PreparedPlan {
    root_id: i64,
    previous_revision: i64,
    creates: Vec<CreateNode>,
    replacements: Vec<ReplaceNode>,
    deletion_roots: Vec<i64>,
    deleted_ids: Vec<i64>,
    final_content: BTreeMap<NodeRef, NodeContent>,
    initial_parent: BTreeMap<NodeRef, Option<NodeRef>>,
    final_parent: BTreeMap<NodeRef, Option<NodeRef>>,
    initial_orders: BTreeMap<NodeRef, Vec<NodeRef>>,
    declared_orders: Vec<ChildOrder>,
}

struct TreeSnapshot {
    content: BTreeMap<NodeRef, NodeContent>,
    parents: BTreeMap<NodeRef, Option<NodeRef>>,
    orders: BTreeMap<NodeRef, Vec<NodeRef>>,
}

/// Parse one strict ingestion document.
pub fn parse_plan(document: &str) -> Result<IngestPlan, AppError> {
    serde_json::from_str(document).map_err(|error| {
        AppError::invalid(
            "invalid_ingestion",
            format!("invalid ingestion document: {error}"),
        )
    })
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::invalid("invalid_ingestion", message)
}

fn topology(message: impl Into<String>) -> AppError {
    AppError::conflict("invalid_topology", message)
}

fn validate_content(content: &NodeContent) -> Result<(), AppError> {
    if content.title.is_empty() {
        return Err(invalid("a node title cannot be empty"));
    }
    if content.title.trim() != content.title {
        return Err(invalid(
            "ingested node titles cannot have leading or trailing whitespace",
        ));
    }
    match (content.kind, &content.source) {
        (NodeKind::Topic, None) => {}
        (NodeKind::Topic, Some(_)) => {
            return Err(invalid("a topic node must have a null source value"));
        }
        (NodeKind::Source, None) => {
            return Err(invalid("a source node must include a source object"));
        }
        (NodeKind::Source, Some(source)) => {
            validate_checksum(source.checksum.as_deref())?;
            validate_captured_at(source.captured_at.as_deref())?;
        }
    }
    Ok(())
}

fn validate_checksum(value: Option<&str>) -> Result<(), AppError> {
    let Some(value) = value else {
        return Ok(());
    };
    let Some((algorithm, digest)) = value.split_once(':') else {
        return Err(invalid(
            "a source checksum must have an algorithm prefix such as sha256:...",
        ));
    };
    let valid_algorithm = !algorithm.is_empty()
        && algorithm
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if !valid_algorithm || digest.trim().is_empty() {
        return Err(invalid(
            "a source checksum must have an algorithm prefix such as sha256:...",
        ));
    }
    Ok(())
}

fn validate_captured_at(value: Option<&str>) -> Result<(), AppError> {
    if let Some(value) = value {
        OffsetDateTime::parse(value, &Rfc3339)
            .map_err(|_| invalid("a source captured_at value must be an RFC 3339 timestamp"))?;
    }
    Ok(())
}

fn content_from_node(node: &Node) -> NodeContent {
    NodeContent {
        kind: node.kind,
        title: node.title.clone(),
        body: node.body.clone(),
        source: node.source.as_ref().map(|source| SourceContent {
            locator: source.locator.clone(),
            media_type: source.media_type.clone(),
            checksum: source.checksum.clone(),
            captured_at: source.captured_at.clone(),
        }),
    }
}

fn load_tree(connection: &Connection, root_id: i64) -> Result<TreeSnapshot, AppError> {
    let mut content = BTreeMap::new();
    let mut parents = BTreeMap::new();
    let mut orders = BTreeMap::new();
    for (node, _) in tree::subtree_with_depth(connection, root_id, None)? {
        let key = NodeRef::Existing(node.id);
        let parent = node.parent_id.map(NodeRef::Existing);
        orders.entry(key.clone()).or_insert_with(Vec::new);
        if let Some(parent) = &parent {
            orders
                .entry(parent.clone())
                .or_insert_with(Vec::new)
                .push(key.clone());
        }
        content.insert(key.clone(), content_from_node(&node));
        parents.insert(key.clone(), parent);
    }
    Ok(TreeSnapshot {
        content,
        parents,
        orders,
    })
}

fn ensure_existing_in_tree(
    connection: &Connection,
    content: &BTreeMap<NodeRef, NodeContent>,
    node_id: i64,
) -> Result<(), AppError> {
    if content.contains_key(&NodeRef::Existing(node_id)) {
        return Ok(());
    }
    match tree::get_node(connection, node_id) {
        Ok(_) => Err(AppError::conflict(
            "cross_tree_move_not_supported",
            format!("node {node_id} is outside the ingestion tree"),
        )),
        Err(error) => Err(error),
    }
}

fn ensure_reference_known(
    connection: &Connection,
    initial_content: &BTreeMap<NodeRef, NodeContent>,
    created_refs: &BTreeSet<String>,
    reference: &NodeRef,
) -> Result<(), AppError> {
    match reference {
        NodeRef::Existing(node_id) => {
            ensure_existing_in_tree(connection, initial_content, *node_id)
        }
        NodeRef::Created(local_ref) if created_refs.contains(local_ref) => Ok(()),
        NodeRef::Created(local_ref) => Err(invalid(format!(
            "ingestion reference {local_ref:?} was not declared by create_nodes"
        ))),
    }
}

#[allow(clippy::too_many_lines)]
fn prepare(transaction: &Transaction<'_>, plan: IngestPlan) -> Result<PreparedPlan, AppError> {
    let previous_revision = tree::library_revision(transaction)?;
    if plan.base_revision != previous_revision {
        return Err(AppError::conflict(
            "stale_revision",
            format!(
                "ingestion expected library revision {}, but the current revision is {previous_revision}",
                plan.base_revision
            ),
        ));
    }
    if plan.create_nodes.is_empty()
        && plan.replace_nodes.is_empty()
        && plan.delete_subtrees.is_empty()
        && plan.child_orders.is_empty()
    {
        return Err(invalid(
            "an ingestion document must declare at least one change",
        ));
    }

    let TreeSnapshot {
        content: initial_content,
        parents: initial_parent,
        orders: initial_orders,
    } = load_tree(transaction, plan.tree_root_id)?;
    let root_key = NodeRef::Existing(plan.tree_root_id);

    let mut created_refs = BTreeSet::new();
    for creation in &plan.create_nodes {
        if creation.reference.trim().is_empty() {
            return Err(invalid("create_nodes references cannot be empty"));
        }
        if creation.reference.trim() != creation.reference {
            return Err(invalid(
                "create_nodes references cannot have leading or trailing whitespace",
            ));
        }
        if !created_refs.insert(creation.reference.clone()) {
            return Err(invalid(format!(
                "create_nodes reference {:?} is duplicated",
                creation.reference
            )));
        }
        validate_content(&creation.node)?;
    }

    let mut deletion_roots = Vec::new();
    let mut deleted_ids = Vec::new();
    let mut deleted_keys = BTreeSet::new();
    for deletion in &plan.delete_subtrees {
        ensure_existing_in_tree(transaction, &initial_content, deletion.root_id)?;
        if deletion.root_id == plan.tree_root_id {
            return Err(AppError::conflict(
                "root_delete_not_allowed",
                "ingestion cannot delete its tree root",
            ));
        }
        let actual_ids = tree::subtree_ids(transaction, deletion.root_id)?;
        if actual_ids != deletion.expected_node_ids {
            return Err(AppError::conflict(
                "subtree_changed",
                format!(
                    "subtree {} does not match expected_node_ids",
                    deletion.root_id
                ),
            ));
        }
        for node_id in &actual_ids {
            if !deleted_keys.insert(NodeRef::Existing(*node_id)) {
                return Err(invalid("delete_subtrees entries cannot overlap"));
            }
        }
        deletion_roots.push(deletion.root_id);
        deleted_ids.extend(actual_ids);
    }

    let mut replacements_seen = BTreeSet::new();
    for replacement in &plan.replace_nodes {
        ensure_existing_in_tree(transaction, &initial_content, replacement.id)?;
        if !replacements_seen.insert(replacement.id) {
            return Err(invalid(format!(
                "node {} is replaced more than once",
                replacement.id
            )));
        }
        if deleted_keys.contains(&NodeRef::Existing(replacement.id)) {
            return Err(invalid(format!(
                "node {} cannot be both replaced and deleted",
                replacement.id
            )));
        }
        validate_content(&replacement.node)?;
    }

    let mut declared_parents = BTreeSet::new();
    for order in &plan.child_orders {
        ensure_reference_known(transaction, &initial_content, &created_refs, &order.parent)?;
        if !declared_parents.insert(order.parent.clone()) {
            return Err(invalid(format!(
                "a parent has more than one child_orders entry: {:?}",
                order.parent
            )));
        }
        let mut children_seen = BTreeSet::new();
        for child in &order.children {
            ensure_reference_known(transaction, &initial_content, &created_refs, child)?;
            if !children_seen.insert(child.clone()) {
                return Err(topology(format!(
                    "a child appears more than once beneath parent {:?}",
                    order.parent
                )));
            }
        }
    }

    for deletion_root in &deletion_roots {
        let key = NodeRef::Existing(*deletion_root);
        let old_parent = initial_parent.get(&key).and_then(Clone::clone);
        if old_parent.as_ref().is_some_and(|parent| {
            !deleted_keys.contains(parent) && !declared_parents.contains(parent)
        }) {
            return Err(AppError::conflict(
                "incomplete_child_order",
                format!("deleting subtree {deletion_root} changes an undeclared parent child list"),
            ));
        }
    }

    for order in &plan.child_orders {
        for child in &order.children {
            if let NodeRef::Existing(node_id) = child {
                let old_parent = initial_parent.get(child).and_then(Clone::clone);
                if old_parent.as_ref() != Some(&order.parent)
                    && old_parent.as_ref().is_some_and(|parent| {
                        !deleted_keys.contains(parent) && !declared_parents.contains(parent)
                    })
                {
                    return Err(AppError::conflict(
                        "incomplete_child_order",
                        format!(
                            "moving node {node_id} changes an undeclared old parent child list"
                        ),
                    ));
                }
            }
        }
    }

    let mut final_content = initial_content.clone();
    for deleted in &deleted_keys {
        final_content.remove(deleted);
    }
    for creation in &plan.create_nodes {
        final_content.insert(
            NodeRef::Created(creation.reference.clone()),
            creation.node.clone(),
        );
    }
    for replacement in &plan.replace_nodes {
        final_content.insert(NodeRef::Existing(replacement.id), replacement.node.clone());
    }
    for content in final_content.values() {
        validate_content(content)?;
    }

    for parent in &declared_parents {
        if !final_content.contains_key(parent) {
            return Err(topology(format!(
                "deleted node {parent:?} cannot declare a final child order"
            )));
        }
    }

    let declared_order_map = plan
        .child_orders
        .iter()
        .map(|order| (order.parent.clone(), order.children.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut final_orders = BTreeMap::new();
    for key in final_content.keys() {
        let order = declared_order_map
            .get(key)
            .cloned()
            .unwrap_or_else(|| initial_orders.get(key).cloned().unwrap_or_default());
        final_orders.insert(key.clone(), order);
    }

    let mut final_parent = BTreeMap::new();
    final_parent.insert(root_key.clone(), None);
    for (parent, children) in &final_orders {
        for child in children {
            if child == &root_key {
                return Err(AppError::conflict(
                    "root_move_not_allowed",
                    "ingestion cannot move its tree root",
                ));
            }
            if !final_content.contains_key(child) {
                let code = if declared_parents.contains(parent) {
                    "invalid_topology"
                } else {
                    "incomplete_child_order"
                };
                return Err(AppError::conflict(
                    code,
                    format!("parent {parent:?} contains a deleted or unknown child {child:?}"),
                ));
            }
            if final_parent
                .insert(child.clone(), Some(parent.clone()))
                .is_some()
            {
                return Err(topology(format!(
                    "child {child:?} appears beneath more than one parent"
                )));
            }
        }
    }

    for key in final_content.keys() {
        if !final_parent.contains_key(key) {
            return Err(AppError::conflict(
                "incomplete_child_order",
                format!("node {key:?} has no declared final parent"),
            ));
        }
    }

    for (parent, children) in &final_orders {
        let Some(parent_content) = final_content.get(parent) else {
            return Err(topology(format!("final parent {parent:?} does not exist")));
        };
        if parent_content.kind == NodeKind::Source && !children.is_empty() {
            return Err(AppError::conflict(
                "source_cannot_have_children",
                format!("source node {parent:?} cannot have children"),
            ));
        }
    }

    let mut reachable = BTreeSet::new();
    let mut pending = vec![root_key];
    while let Some(node) = pending.pop() {
        if !reachable.insert(node.clone()) {
            return Err(AppError::conflict(
                "would_create_cycle",
                "the ingestion child orders would create a cycle",
            ));
        }
        if let Some(children) = final_orders.get(&node) {
            pending.extend(children.iter().cloned());
        }
    }
    if reachable.len() != final_content.len() {
        return Err(AppError::conflict(
            "would_create_cycle",
            "the ingestion child orders do not form one rooted tree",
        ));
    }

    Ok(PreparedPlan {
        root_id: plan.tree_root_id,
        previous_revision,
        creates: plan.create_nodes,
        replacements: plan.replace_nodes,
        deletion_roots,
        deleted_ids,
        final_content,
        initial_parent,
        final_parent,
        initial_orders,
        declared_orders: plan.child_orders,
    })
}

fn timestamp() -> Result<String, AppError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| AppError::unexpected("timestamp_error", error.to_string()))
}

fn resolve(reference: &NodeRef, created: &BTreeMap<String, i64>) -> Result<i64, AppError> {
    match reference {
        NodeRef::Existing(node_id) => Ok(*node_id),
        NodeRef::Created(local_ref) => created.get(local_ref).copied().ok_or_else(|| {
            AppError::unexpected(
                "unresolved_ingestion_reference",
                format!("validated ingestion reference {local_ref:?} was not created"),
            )
        }),
    }
}

fn temporary_position(start: i64, offset: usize) -> Result<i64, AppError> {
    let offset = i64::try_from(offset)
        .map_err(|_| AppError::database("too_many_nodes", "too many nodes to assign positions"))?;
    start.checked_add(offset).ok_or_else(|| {
        AppError::database(
            "position_overflow",
            "node positions are too large to stage ingestion",
        )
    })
}

fn final_position(ordinal: usize) -> Result<i64, AppError> {
    i64::try_from(ordinal)
        .ok()
        .and_then(|value| value.checked_mul(POSITION_STEP))
        .ok_or_else(|| {
            AppError::database(
                "position_overflow",
                "a declared child list is too large to assign positions",
            )
        })
}

fn insert_source(
    transaction: &Transaction<'_>,
    node_id: i64,
    source: &SourceContent,
) -> Result<(), AppError> {
    transaction.execute(
        "INSERT INTO sources(node_id, locator, media_type, checksum, captured_at) \
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![
            node_id,
            source.locator,
            source.media_type,
            source.checksum,
            source.captured_at
        ],
    )?;
    Ok(())
}

fn replace_content(
    transaction: &Transaction<'_>,
    replacement: &ReplaceNode,
    updated_at: &str,
) -> Result<(), AppError> {
    transaction.execute(
        "UPDATE nodes SET kind = ?1, title = ?2, body = ?3, updated_at = ?4 WHERE id = ?5",
        params![
            replacement.node.kind.as_str(),
            replacement.node.title,
            replacement.node.body,
            updated_at,
            replacement.id
        ],
    )?;
    transaction.execute("DELETE FROM sources WHERE node_id = ?1", [replacement.id])?;
    if let Some(source) = &replacement.node.source {
        insert_source(transaction, replacement.id, source)?;
    }
    Ok(())
}

fn affected_existing_nodes(prepared: &PreparedPlan) -> BTreeSet<i64> {
    let deleted = prepared
        .deleted_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut affected = BTreeSet::new();
    for order in &prepared.declared_orders {
        if let Some(old_children) = prepared.initial_orders.get(&order.parent) {
            for child in old_children {
                if let NodeRef::Existing(node_id) = child
                    && !deleted.contains(node_id)
                {
                    affected.insert(*node_id);
                }
            }
        }
        for child in &order.children {
            if let NodeRef::Existing(node_id) = child
                && !deleted.contains(node_id)
            {
                affected.insert(*node_id);
            }
        }
    }
    affected
}

fn moved_nodes(
    prepared: &PreparedPlan,
    created: &BTreeMap<String, i64>,
) -> Result<Vec<MoveReceipt>, AppError> {
    let mut moved = Vec::new();
    for (node, new_parent) in &prepared.final_parent {
        let NodeRef::Existing(node_id) = node else {
            continue;
        };
        let Some(old_parent) = prepared.initial_parent.get(node) else {
            continue;
        };
        if old_parent == new_parent {
            continue;
        }
        let (Some(old_parent), Some(new_parent)) = (old_parent, new_parent) else {
            continue;
        };
        moved.push(MoveReceipt {
            node_id: *node_id,
            from_parent: resolve(old_parent, created)?,
            to_parent: resolve(new_parent, created)?,
        });
    }
    Ok(moved)
}

fn resolved_orders(
    prepared: &PreparedPlan,
    created: &BTreeMap<String, i64>,
) -> Result<Vec<ResolvedChildOrder>, AppError> {
    prepared
        .declared_orders
        .iter()
        .map(|order| {
            Ok(ResolvedChildOrder {
                parent: resolve(&order.parent, created)?,
                children: order
                    .children
                    .iter()
                    .map(|child| resolve(child, created))
                    .collect::<Result<Vec<_>, _>>()?,
            })
        })
        .collect()
}

fn verify_postconditions(
    transaction: &Transaction<'_>,
    prepared: &PreparedPlan,
    created: &BTreeMap<String, i64>,
    orders: &[ResolvedChildOrder],
) -> Result<(), AppError> {
    for (reference, expected_content) in &prepared.final_content {
        let node_id = resolve(reference, created)?;
        let actual = tree::get_node(transaction, node_id)?;
        if content_from_node(&actual) != *expected_content {
            return Err(AppError::database(
                "ingestion_postcondition_failed",
                format!("node {node_id} content does not match the validated ingestion"),
            ));
        }
        let expected_parent = prepared
            .final_parent
            .get(reference)
            .and_then(Clone::clone)
            .as_ref()
            .map(|parent| resolve(parent, created))
            .transpose()?;
        if actual.parent_id != expected_parent {
            return Err(AppError::database(
                "ingestion_postcondition_failed",
                format!("node {node_id} parent does not match the validated ingestion"),
            ));
        }
    }
    for order in orders {
        let children = tree::children(transaction, order.parent)?;
        let actual_ids = children.iter().map(|child| child.id).collect::<Vec<_>>();
        if actual_ids != order.children {
            return Err(AppError::database(
                "ingestion_postcondition_failed",
                format!(
                    "parent {} child order does not match the validated ingestion",
                    order.parent
                ),
            ));
        }
        for (ordinal, child) in children.iter().enumerate() {
            if child.position != final_position(ordinal)? {
                return Err(AppError::database(
                    "ingestion_postcondition_failed",
                    format!(
                        "node {} position does not encode its declared order",
                        child.id
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Validate and apply one ingestion document inside the caller's transaction.
#[allow(clippy::too_many_lines)]
pub fn apply(transaction: &Transaction<'_>, plan: IngestPlan) -> Result<IngestOutput, AppError> {
    let prepared = prepare(transaction, plan)?;
    let updated_at = timestamp()?;
    let staged_nodes = affected_existing_nodes(&prepared);
    let temporary_start = if prepared.creates.is_empty() && staged_nodes.is_empty() {
        0
    } else {
        let max_position =
            transaction.query_row("SELECT COALESCE(MAX(position), 0) FROM nodes", [], |row| {
                row.get::<_, i64>(0)
            })?;
        let mut max_final_position = 0;
        for order in &prepared.declared_orders {
            if let Some(last_ordinal) = order.children.len().checked_sub(1) {
                max_final_position = max_final_position.max(final_position(last_ordinal)?);
            }
        }
        max_position
            .max(max_final_position)
            .checked_add(POSITION_STEP)
            .ok_or_else(|| {
                AppError::database(
                    "position_overflow",
                    "node positions are too large to stage ingestion",
                )
            })?
    };

    let mut created = BTreeMap::new();
    for (offset, creation) in prepared.creates.iter().enumerate() {
        let position = temporary_position(temporary_start, offset)?;
        transaction.execute(
            "INSERT INTO nodes(parent_id, kind, title, body, position, created_at, updated_at) \
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                prepared.root_id,
                creation.node.kind.as_str(),
                creation.node.title,
                creation.node.body,
                position,
                updated_at
            ],
        )?;
        let node_id = transaction.last_insert_rowid();
        if let Some(source) = &creation.node.source {
            insert_source(transaction, node_id, source)?;
        }
        created.insert(creation.reference.clone(), node_id);
    }

    for replacement in &prepared.replacements {
        replace_content(transaction, replacement, &updated_at)?;
    }
    for root_id in &prepared.deletion_roots {
        transaction.execute("DELETE FROM nodes WHERE id = ?1", [root_id])?;
    }

    for (offset, node_id) in staged_nodes.iter().enumerate() {
        let stage_offset = prepared
            .creates
            .len()
            .checked_add(offset)
            .ok_or_else(|| AppError::database("too_many_nodes", "too many ingested nodes"))?;
        let position = temporary_position(temporary_start, stage_offset)?;
        transaction.execute(
            "UPDATE nodes SET position = ?1 WHERE id = ?2",
            params![position, node_id],
        )?;
    }

    for order in &prepared.declared_orders {
        let parent_id = resolve(&order.parent, &created)?;
        for (ordinal, child) in order.children.iter().enumerate() {
            let child_id = resolve(child, &created)?;
            transaction.execute(
                "UPDATE nodes SET parent_id = ?1, position = ?2 WHERE id = ?3",
                params![parent_id, final_position(ordinal)?, child_id],
            )?;
        }
    }

    let moved = moved_nodes(&prepared, &created)?;
    for movement in &moved {
        transaction.execute(
            "UPDATE nodes SET updated_at = ?1 WHERE id = ?2",
            params![updated_at, movement.node_id],
        )?;
    }
    let final_child_orders = resolved_orders(&prepared, &created)?;
    verify_postconditions(transaction, &prepared, &created, &final_child_orders)?;
    index::rebuild_subtree(transaction, prepared.root_id)?;
    let new_revision = tree::bump_library_revision(transaction)?;

    Ok(IngestOutput {
        previous_revision: prepared.previous_revision,
        new_revision,
        created,
        replaced_node_ids: prepared
            .replacements
            .iter()
            .map(|replacement| replacement.id)
            .collect(),
        moved,
        deleted_node_ids: prepared.deleted_ids,
        final_child_orders,
    })
}
