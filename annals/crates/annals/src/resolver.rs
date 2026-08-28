#![allow(clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
use crate::change::parse_reconciliation_value;
use crate::change::{
    ChangeOperation, ConceptSelector, EvidenceDisposition, EvidenceSelector, Reconciliation,
    ReconciliationContractError, created_id_bindings, insert_request, load_request,
    parse_reconciliation, reserve_create_ids,
};
use crate::corpus::{
    ReconciliationRecord, Snapshot, SnapshotConcept, SnapshotEdge, SnapshotEvidence, Work,
    insert_commit, insert_reconciliation, revision, snapshot_at, validate_snapshot,
};
use crate::error::AppError;
use crate::index;
use crate::model::{ConceptId, ConceptReference};

pub(crate) const MAX_EVIDENCE_MATCHES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolvedReconciliation {
    pub base_revision: i64,
    pub operations: Vec<ResolvedOperation>,
    pub resulting_snapshot: Snapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolvedEvidence {
    pub quote: String,
    pub occurrence_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ResolvedOperation {
    CreateConcept {
        concept: ConceptReference,
        parents: Vec<ConceptReference>,
        evidence: Vec<ResolvedEvidence>,
    },
    AddParent {
        concept: ConceptReference,
        parent: ConceptReference,
    },
    RemoveParent {
        concept: ConceptReference,
        parent: ConceptReference,
    },
    AddEvidence {
        concept: ConceptReference,
        evidence: Vec<ResolvedEvidence>,
    },
    RemoveEvidence {
        concept: ConceptReference,
        evidence: Vec<ResolvedEvidence>,
    },
    RewordConcept {
        id: ConceptId,
        before: String,
        after: String,
        evidence_disposition: EvidenceDisposition,
    },
    RetireConcept {
        concept: ConceptReference,
        #[serde(skip_serializing_if = "Option::is_none")]
        replacement: Option<ConceptReference>,
        removed_parents: Vec<ConceptReference>,
        removed_children: Vec<ConceptReference>,
    },
}

pub(crate) fn submit_document(
    connection: &mut Connection,
    work: &Work,
    base_revision: i64,
    document: &str,
    actor: &str,
    model_run_id: Option<i64>,
) -> Result<ReconciliationRecord, AppError> {
    let reconciliation = parse_reconciliation(document).map_err(|error| contract_error(&error))?;
    submit(
        connection,
        work,
        base_revision,
        &reconciliation,
        actor,
        model_run_id,
        None,
    )
}

#[cfg(test)]
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn submit_value(
    connection: &mut Connection,
    work: &Work,
    base_revision: i64,
    value: Value,
    actor: &str,
    model_run_id: Option<i64>,
) -> Result<ReconciliationRecord, AppError> {
    let reconciliation =
        parse_reconciliation_value(value).map_err(|error| contract_error(&error))?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let record = submit_parsed(
        &transaction,
        work,
        base_revision,
        &reconciliation,
        actor,
        model_run_id,
        None,
    )?;
    transaction.commit()?;
    Ok(record)
}

#[allow(clippy::too_many_arguments)]
fn submit(
    connection: &mut Connection,
    work: &Work,
    base_revision: i64,
    reconciliation: &Reconciliation,
    actor: &str,
    model_run_id: Option<i64>,
    reconciliation_draft_id: Option<i64>,
) -> Result<ReconciliationRecord, AppError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let record = submit_parsed(
        &transaction,
        work,
        base_revision,
        reconciliation,
        actor,
        model_run_id,
        reconciliation_draft_id,
    )?;
    transaction.commit()?;
    Ok(record)
}

#[allow(clippy::too_many_arguments)]
fn submit_parsed(
    connection: &Transaction<'_>,
    work: &Work,
    base_revision: i64,
    reconciliation: &Reconciliation,
    actor: &str,
    model_run_id: Option<i64>,
    reconciliation_draft_id: Option<i64>,
) -> Result<ReconciliationRecord, AppError> {
    let base = snapshot_at(connection, base_revision)?;
    let created_ids = reserve_create_ids(connection, reconciliation)?;
    let resolved = resolve(
        connection,
        work,
        base_revision,
        &base,
        reconciliation,
        Some(&created_ids),
    )?;
    let changes_corpus = !snapshots_corpus_equal(&base, &resolved.resulting_snapshot);
    let request_id = insert_request(
        connection,
        work.id,
        base_revision,
        reconciliation,
        &created_ids,
        &crate::corpus::now()?,
    )?;
    insert_reconciliation(
        connection,
        request_id,
        work.id,
        base_revision,
        model_run_id,
        reconciliation_draft_id,
        changes_corpus,
        actor,
    )
}

/// Finalize a model-run draft by linking the reconciliation directly to its
/// normalized request rows.  No request or resolved JSON is copied.
pub(crate) fn submit_stored_request_in_transaction(
    transaction: &Transaction<'_>,
    work: &Work,
    base_revision: i64,
    request_id: i64,
    actor: &str,
    model_run_id: Option<i64>,
    draft_id: Option<i64>,
) -> Result<ReconciliationRecord, AppError> {
    let reconciliation = load_request(transaction, request_id)?;
    let created_ids = created_id_bindings(transaction, request_id)?;
    let base = snapshot_at(transaction, base_revision)?;
    let resolved = resolve(
        transaction,
        work,
        base_revision,
        &base,
        &reconciliation,
        Some(&created_ids),
    )?;
    let changes_corpus = !snapshots_corpus_equal(&base, &resolved.resulting_snapshot);
    insert_reconciliation(
        transaction,
        request_id,
        work.id,
        base_revision,
        model_run_id,
        draft_id,
        changes_corpus,
        actor,
    )
}

pub(crate) fn validate_record(
    connection: &Connection,
    record: &ReconciliationRecord,
) -> Result<ResolvedReconciliation, AppError> {
    if record.status != "pending" {
        return Err(AppError::conflict(
            "nothing_to_apply",
            "the selected reconciliation is not pending",
        ));
    }
    let head_revision = revision(connection)?;
    if head_revision != record.base_revision {
        return Err(stale_change(record.base_revision, head_revision));
    }
    let base = snapshot_at(connection, record.base_revision)?;
    let replayed = replay_record(connection, record)?;
    if snapshots_corpus_equal(&base, &replayed.resulting_snapshot) {
        return Err(AppError::database(
            "invalid_pending_reconciliation",
            "a pending reconciliation must project a corpus transition",
        ));
    }
    validate_snapshot(connection, &replayed.resulting_snapshot).map_err(|error| {
        AppError::database(
            "invalid_pending_reconciliation",
            format!("the replayed resulting corpus is invalid: {error}"),
        )
    })?;
    Ok(replayed)
}

pub(crate) fn replay_record(
    connection: &Connection,
    record: &ReconciliationRecord,
) -> Result<ResolvedReconciliation, AppError> {
    let reconciliation = load_request(connection, record.request_id)?;
    let work = crate::corpus::get_work_by_id(connection, record.work_id)?;
    let base = snapshot_at(connection, record.base_revision)?;
    let created_ids = created_id_bindings(connection, record.request_id)?;
    resolve(
        connection,
        &work,
        record.base_revision,
        &base,
        &reconciliation,
        Some(&created_ids),
    )
}

pub(crate) fn apply_record(
    connection: &mut Connection,
    record: &ReconciliationRecord,
) -> Result<i64, AppError> {
    apply_record_with_ingestion(connection, record, None)
}

pub(crate) fn apply_record_for_ingestion(
    connection: &mut Connection,
    record: &ReconciliationRecord,
    ingestion_id: i64,
) -> Result<i64, AppError> {
    apply_record_with_ingestion(connection, record, Some(ingestion_id))
}

fn apply_record_with_ingestion(
    connection: &mut Connection,
    record: &ReconciliationRecord,
    ingestion_id: Option<i64>,
) -> Result<i64, AppError> {
    if record.status != "pending" {
        return Err(AppError::conflict(
            "nothing_to_apply",
            "the selected reconciliation is not pending",
        ));
    }
    let resolved = validate_record(connection, record)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current_status = transaction
        .query_row(
            "SELECT status FROM reconciliations WHERE id = ?1",
            [record.id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if current_status.as_deref() != Some("pending") {
        return Err(AppError::conflict(
            "nothing_to_apply",
            "the selected reconciliation is no longer pending",
        ));
    }
    let head_revision = revision(&transaction)?;
    if head_revision != record.base_revision {
        return Err(stale_change(record.base_revision, head_revision));
    }
    let revalidated = replay_record(&transaction, record)?;
    if revalidated != resolved {
        return Err(AppError::conflict(
            "reconciliation_resolution_changed",
            "the stored reconciliation no longer resolves to the same transition",
        ));
    }
    let new_revision = head_revision.checked_add(1).ok_or_else(|| {
        AppError::database("revision_overflow", "the corpus revision is too large")
    })?;
    insert_commit(
        &transaction,
        new_revision,
        Some(record.id),
        "change",
        None,
        &resolved.resulting_snapshot,
        &record.actor,
    )?;
    let updated = transaction.execute(
        "UPDATE reconciliations SET status = 'applied', applied_revision = ?1 \
         WHERE id = ?2 AND status = 'pending'",
        params![new_revision, record.id],
    )?;
    if updated != 1 {
        return Err(AppError::conflict(
            "nothing_to_apply",
            "the selected reconciliation is no longer pending",
        ));
    }
    if let Some(ingestion_id) = ingestion_id {
        crate::ingestion::complete(&transaction, ingestion_id, "applied", Some(new_revision))?;
    }
    transaction.commit()?;
    Ok(new_revision)
}

fn stale_change(base_revision: i64, head_revision: i64) -> AppError {
    AppError::conflict(
        "stale_change",
        format!(
            "the reconciliation examined revision {base_revision}, but HEAD is revision {head_revision}"
        ),
    )
}

#[derive(Clone, Copy)]
enum ResolvedSelectors {
    Create {
        concept: i64,
    },
    Edge {
        concept: i64,
        parent: i64,
    },
    One {
        concept: i64,
    },
    Retire {
        concept: i64,
        replacement: Option<i64>,
    },
}

fn resolve(
    connection: &Connection,
    work: &Work,
    base_revision: i64,
    base: &Snapshot,
    reconciliation: &Reconciliation,
    recorded_create_ids: Option<&HashMap<String, i64>>,
) -> Result<ResolvedReconciliation, AppError> {
    validate_snapshot(connection, base).map_err(|error| {
        AppError::database(
            "invalid_history_snapshot",
            format!("revision {base_revision} contains an invalid corpus: {error}"),
        )
    })?;
    let operations = reconciliation.operations();
    let base_ids = base
        .concepts
        .iter()
        .map(|concept| concept.id)
        .collect::<BTreeSet<_>>();
    let mut local_ids = HashMap::new();
    let mut allocated_ids = BTreeSet::new();
    for operation in operations {
        if let ChangeOperation::CreateConcept { handle, .. } = operation {
            let id = recorded_create_ids
                .and_then(|recorded| recorded.get(handle).copied())
                .ok_or_else(|| {
                    AppError::database(
                        "concept_binding_missing",
                        format!("created concept ref {handle:?} has no reserved public ID"),
                    )
                })?;
            if id <= 0 || base_ids.contains(&id) || !allocated_ids.insert(id) {
                return Err(AppError::database(
                    "invalid_resolved_reconciliation",
                    format!("created concept ref {handle:?} has invalid recorded ID {id}"),
                ));
            }
            local_ids.insert(handle.clone(), id);
        }
    }
    if recorded_create_ids.is_some_and(|recorded| recorded.len() != local_ids.len()) {
        return Err(AppError::database(
            "invalid_resolved_reconciliation",
            "the stored reconciliation has unexpected created-concept IDs",
        ));
    }
    let resolve_selector = |selector: &ConceptSelector| -> Result<i64, AppError> {
        match selector {
            ConceptSelector::Existing { id } => {
                let storage_id = id.storage_id();
                if base_ids.contains(&storage_id) {
                    Ok(storage_id)
                } else {
                    Err(AppError::not_found(
                        "concept_not_found",
                        format!("{id} was not found at revision {base_revision}"),
                    ))
                }
            }
            ConceptSelector::New { handle } => local_ids.get(handle).copied().ok_or_else(|| {
                invalid_change(format!("new concept ref {handle:?} was not declared"))
            }),
        }
    };
    let selectors = operations
        .iter()
        .map(|operation| resolve_operation_selectors(operation, &resolve_selector))
        .collect::<Result<Vec<_>, AppError>>()?;

    let mut result = base.clone();
    for (operation, selector) in operations.iter().zip(&selectors) {
        if let (
            ChangeOperation::CreateConcept { label, .. },
            ResolvedSelectors::Create { concept },
        ) = (operation, selector)
        {
            result.concepts.push(SnapshotConcept {
                id: *concept,
                label: label.clone(),
            });
        }
    }

    let created = local_ids.values().copied().collect::<BTreeSet<_>>();
    let mut retired = BTreeSet::new();
    let mut reworded = BTreeSet::new();
    for (operation, selector) in operations.iter().zip(&selectors) {
        match (operation, selector) {
            (ChangeOperation::RetireConcept { .. }, ResolvedSelectors::Retire { concept, .. }) => {
                if created.contains(concept) {
                    return Err(invalid_change(
                        "a concept cannot be created and retired in one reconciliation",
                    ));
                }
                if !retired.insert(*concept) {
                    return Err(invalid_change(
                        "a concept cannot be retired more than once in one reconciliation",
                    ));
                }
            }
            (ChangeOperation::RewordConcept { .. }, ResolvedSelectors::One { concept }) => {
                if created.contains(concept) {
                    return Err(invalid_change(
                        "a newly created concept already supplies its final wording",
                    ));
                }
                if !reworded.insert(*concept) {
                    return Err(invalid_change(
                        "a concept cannot be reworded more than once in one reconciliation",
                    ));
                }
            }
            _ => {}
        }
    }

    let base_edges = base
        .edges
        .iter()
        .map(|edge| (edge.parent_id, edge.child_id))
        .collect::<BTreeSet<_>>();
    let mut edge_adds = BTreeSet::new();
    let mut edge_removes = BTreeSet::new();
    for (operation, selector) in operations.iter().zip(&selectors) {
        match (operation, selector) {
            (
                ChangeOperation::CreateConcept { parents, .. },
                ResolvedSelectors::Create { concept },
            ) => {
                let mut seen = BTreeSet::new();
                for parent in parents {
                    let parent = resolve_selector(parent)?;
                    if !seen.insert(parent) {
                        return Err(invalid_change(
                            "a created concept cannot name the same parent more than once",
                        ));
                    }
                    edge_adds.insert((parent, *concept));
                }
            }
            (ChangeOperation::AddParent { .. }, ResolvedSelectors::Edge { concept, parent }) => {
                edge_adds.insert((*parent, *concept));
            }
            (ChangeOperation::RemoveParent { .. }, ResolvedSelectors::Edge { concept, parent }) => {
                let edge = (*parent, *concept);
                if !base_edges.contains(&edge) {
                    return Err(AppError::conflict(
                        "parent_edge_not_found",
                        format!(
                            "the direct parent edge {} -> {} does not exist",
                            cid(*parent)?,
                            cid(*concept)?
                        ),
                    ));
                }
                if !edge_removes.insert(edge) {
                    return Err(invalid_change(
                        "the same parent edge cannot be removed more than once",
                    ));
                }
            }
            _ => {}
        }
    }
    if let Some(edge) = edge_adds.intersection(&edge_removes).next() {
        return Err(invalid_change(format!(
            "the same parent edge {} -> {} cannot be added and removed",
            cid(edge.0)?,
            cid(edge.1)?
        )));
    }

    for (operation, selector) in operations.iter().zip(&selectors) {
        match selector {
            ResolvedSelectors::Create { .. } => {}
            ResolvedSelectors::Edge { concept, parent } => {
                reject_retired(&retired, *concept)?;
                reject_retired(&retired, *parent)?;
                if concept == parent {
                    return Err(AppError::conflict(
                        "would_create_cycle",
                        format!("{} cannot be its own parent", cid(*concept)?),
                    ));
                }
            }
            ResolvedSelectors::One { concept } => reject_retired(&retired, *concept)?,
            ResolvedSelectors::Retire {
                concept,
                replacement,
            } => {
                if replacement == &Some(*concept) {
                    return Err(invalid_change("a retired concept cannot replace itself"));
                }
                if replacement.is_some_and(|id| retired.contains(&id)) {
                    return Err(invalid_change(
                        "a retirement replacement must survive the reconciliation",
                    ));
                }
            }
        }
        if let ChangeOperation::CreateConcept { parents, .. } = operation {
            for parent in parents {
                reject_retired(&retired, resolve_selector(parent)?)?;
            }
        }
    }

    let mut edges = base_edges;
    edges.extend(edge_adds.iter().copied());
    for edge in &edge_removes {
        edges.remove(edge);
    }
    edges.retain(|(parent, child)| !retired.contains(parent) && !retired.contains(child));
    result.edges = edges
        .iter()
        .map(|(parent_id, child_id)| SnapshotEdge {
            parent_id: *parent_id,
            child_id: *child_id,
        })
        .collect();

    let base_evidence = base
        .evidence
        .iter()
        .map(evidence_key)
        .collect::<BTreeSet<_>>();
    let mut evidence_adds = BTreeSet::new();
    let mut evidence_removes = BTreeSet::new();
    let mut remove_all_evidence = BTreeSet::new();
    for (operation, selector) in operations.iter().zip(&selectors) {
        match (operation, selector) {
            (
                ChangeOperation::CreateConcept { evidence, .. }
                | ChangeOperation::AddEvidence { evidence, .. },
                ResolvedSelectors::Create { concept } | ResolvedSelectors::One { concept },
            ) => {
                reject_retired(&retired, *concept)?;
                for evidence_selector in evidence {
                    evidence_adds.extend(resolve_evidence(work, *concept, evidence_selector)?);
                }
            }
            (
                ChangeOperation::RemoveEvidence { evidence, .. },
                ResolvedSelectors::One { concept },
            ) => {
                reject_retired(&retired, *concept)?;
                for evidence_selector in evidence {
                    let keys = resolve_evidence(work, *concept, evidence_selector)?;
                    if keys.iter().any(|key| !base_evidence.contains(key)) {
                        return Err(AppError::conflict(
                            "evidence_not_attached",
                            format!(
                                "one or more occurrences of the selected quotation are not attached to {}",
                                cid(*concept)?
                            ),
                        ));
                    }
                    if keys.iter().any(|key| evidence_removes.contains(key)) {
                        return Err(invalid_change(
                            "the same evidence cannot be removed more than once",
                        ));
                    }
                    evidence_removes.extend(keys);
                }
            }
            (
                ChangeOperation::RewordConcept {
                    label,
                    evidence_disposition,
                    ..
                },
                ResolvedSelectors::One { concept },
            ) => {
                concept_mut(&mut result, *concept)?.label.clone_from(label);
                if *evidence_disposition == EvidenceDisposition::Remove {
                    remove_all_evidence.insert(*concept);
                }
            }
            _ => {}
        }
    }
    if evidence_adds
        .intersection(&evidence_removes)
        .next()
        .is_some()
    {
        return Err(invalid_change(
            "the same evidence cannot be explicitly added and removed",
        ));
    }

    let mut evidence = base_evidence;
    evidence.retain(|key| {
        !retired.contains(&key.0)
            && !remove_all_evidence.contains(&key.0)
            && !evidence_removes.contains(key)
    });
    evidence.extend(evidence_adds.iter().copied());
    result.evidence = evidence
        .into_iter()
        .map(
            |(concept_id, work_id, start_byte, end_byte)| SnapshotEvidence {
                concept_id,
                work_id,
                start_byte,
                end_byte,
            },
        )
        .collect();

    let before_retirement = result.clone();
    result
        .concepts
        .retain(|concept| !retired.contains(&concept.id));
    result.canonicalize();
    validate_snapshot(connection, &result)?;

    let receipts = operations
        .iter()
        .zip(&selectors)
        .map(|(operation, selector)| {
            resolved_receipt(
                operation,
                *selector,
                base,
                &before_retirement,
                &result,
                work,
                &resolve_selector,
            )
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    if snapshots_corpus_equal(base, &result) {
        result = base.clone();
    }
    Ok(ResolvedReconciliation {
        base_revision,
        operations: receipts,
        resulting_snapshot: result,
    })
}

fn resolve_operation_selectors(
    operation: &ChangeOperation,
    resolve: &impl Fn(&ConceptSelector) -> Result<i64, AppError>,
) -> Result<ResolvedSelectors, AppError> {
    match operation {
        ChangeOperation::CreateConcept { handle, .. } => Ok(ResolvedSelectors::Create {
            concept: resolve(&ConceptSelector::New {
                handle: handle.clone(),
            })?,
        }),
        ChangeOperation::AddParent { concept, parent }
        | ChangeOperation::RemoveParent { concept, parent } => Ok(ResolvedSelectors::Edge {
            concept: resolve(concept)?,
            parent: resolve(parent)?,
        }),
        ChangeOperation::AddEvidence { concept, .. }
        | ChangeOperation::RemoveEvidence { concept, .. }
        | ChangeOperation::RewordConcept { concept, .. } => Ok(ResolvedSelectors::One {
            concept: resolve(concept)?,
        }),
        ChangeOperation::RetireConcept {
            concept,
            replacement,
        } => Ok(ResolvedSelectors::Retire {
            concept: resolve(concept)?,
            replacement: replacement.as_ref().map(resolve).transpose()?,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn resolved_receipt(
    operation: &ChangeOperation,
    selector: ResolvedSelectors,
    base: &Snapshot,
    before_retirement: &Snapshot,
    result: &Snapshot,
    work: &Work,
    resolve: &impl Fn(&ConceptSelector) -> Result<i64, AppError>,
) -> Result<ResolvedOperation, AppError> {
    match (operation, selector) {
        (
            ChangeOperation::CreateConcept {
                parents, evidence, ..
            },
            ResolvedSelectors::Create { concept },
        ) => {
            let mut parent_references = parents
                .iter()
                .map(|parent| concept_reference(before_retirement, resolve(parent)?))
                .collect::<Result<Vec<_>, AppError>>()?;
            sort_references(&mut parent_references);
            parent_references.dedup_by_key(|parent| parent.id);
            Ok(ResolvedOperation::CreateConcept {
                concept: concept_reference(before_retirement, concept)?,
                parents: parent_references,
                evidence: resolved_evidence(work, evidence)?,
            })
        }
        (ChangeOperation::AddParent { .. }, ResolvedSelectors::Edge { concept, parent }) => {
            Ok(ResolvedOperation::AddParent {
                concept: concept_reference(result, concept)?,
                parent: concept_reference(result, parent)?,
            })
        }
        (ChangeOperation::RemoveParent { .. }, ResolvedSelectors::Edge { concept, parent }) => {
            Ok(ResolvedOperation::RemoveParent {
                concept: concept_reference(result, concept)?,
                parent: concept_reference(result, parent)?,
            })
        }
        (ChangeOperation::AddEvidence { evidence, .. }, ResolvedSelectors::One { concept }) => {
            Ok(ResolvedOperation::AddEvidence {
                concept: concept_reference(result, concept)?,
                evidence: resolved_evidence(work, evidence)?,
            })
        }
        (ChangeOperation::RemoveEvidence { evidence, .. }, ResolvedSelectors::One { concept }) => {
            Ok(ResolvedOperation::RemoveEvidence {
                concept: concept_reference(result, concept)?,
                evidence: resolved_evidence(work, evidence)?,
            })
        }
        (
            ChangeOperation::RewordConcept {
                evidence_disposition,
                ..
            },
            ResolvedSelectors::One { concept },
        ) => Ok(ResolvedOperation::RewordConcept {
            id: cid(concept)?,
            before: concept_ref(base, concept)?.label.clone(),
            after: concept_ref(result, concept)?.label.clone(),
            evidence_disposition: *evidence_disposition,
        }),
        (
            ChangeOperation::RetireConcept { .. },
            ResolvedSelectors::Retire {
                concept,
                replacement,
            },
        ) => {
            let concept_view = concept_reference(base, concept)?;
            let mut removed_parents = base
                .edges
                .iter()
                .filter(|edge| edge.child_id == concept)
                .map(|edge| concept_reference(base, edge.parent_id))
                .collect::<Result<Vec<_>, AppError>>()?;
            let mut removed_children = base
                .edges
                .iter()
                .filter(|edge| edge.parent_id == concept)
                .map(|edge| concept_reference(base, edge.child_id))
                .collect::<Result<Vec<_>, AppError>>()?;
            sort_references(&mut removed_parents);
            sort_references(&mut removed_children);
            Ok(ResolvedOperation::RetireConcept {
                concept: concept_view,
                replacement: replacement
                    .map(|id| concept_reference(before_retirement, id))
                    .transpose()?,
                removed_parents,
                removed_children,
            })
        }
        _ => Err(AppError::unexpected(
            "resolver_receipt_mismatch",
            format!(
                "the resolver produced a mismatched receipt for work {:?}",
                work.label
            ),
        )),
    }
}

fn resolved_evidence(
    work: &Work,
    selectors: &[EvidenceSelector],
) -> Result<Vec<ResolvedEvidence>, AppError> {
    selectors
        .iter()
        .map(|selector| {
            Ok(ResolvedEvidence {
                quote: selector.quote.clone(),
                occurrence_count: resolve_quote(work, selector)?.len(),
            })
        })
        .collect()
}

fn resolve_evidence(
    work: &Work,
    concept_id: i64,
    selector: &EvidenceSelector,
) -> Result<Vec<(i64, i64, usize, usize)>, AppError> {
    Ok(resolve_quote(work, selector)?
        .into_iter()
        .map(|(start_byte, end_byte)| (concept_id, work.id, start_byte, end_byte))
        .collect())
}

pub(crate) fn resolve_quote(
    work: &Work,
    selector: &EvidenceSelector,
) -> Result<Vec<(usize, usize)>, AppError> {
    let candidates = matching_quote_ranges(work, selector);
    match candidates.len() {
        0 => Err(AppError::not_found(
            "quote_not_found",
            format!(
                "quotation {:?} was not found in work {:?}",
                selector.quote, work.label
            ),
        )),
        count if count > MAX_EVIDENCE_MATCHES => Err(AppError::conflict(
            "quote_too_many_matches",
            format!(
                "quotation {:?} occurs {count} times in work {:?}; at most {MAX_EVIDENCE_MATCHES} occurrences can be used as evidence; add heading or neighboring context",
                selector.quote, work.label
            ),
        )),
        _ => Ok(candidates),
    }
}

#[must_use]
pub(crate) fn matching_quote_ranges(
    work: &Work,
    selector: &EvidenceSelector,
) -> Vec<(usize, usize)> {
    let mut candidates = exact_quote_ranges(&work.text, &selector.quote);
    if let Some(heading) = &selector.within_heading {
        let normalized = heading
            .iter()
            .map(|segment| index::normalize(segment))
            .collect::<Vec<_>>();
        candidates.retain(|(start, _)| {
            crate::corpus::heading_for_offset(&work.text, *start).is_some_and(|path| {
                path.iter()
                    .map(|segment| index::normalize(segment))
                    .eq(normalized.iter().cloned())
            })
        });
    }
    if let Some(prefix) = &selector.preceded_by {
        candidates.retain(|(start, _)| work.text[..*start].ends_with(prefix));
    }
    if let Some(suffix) = &selector.followed_by {
        candidates.retain(|(_, end)| work.text[*end..].starts_with(suffix));
    }
    candidates
}

fn exact_quote_ranges(text: &str, quote: &str) -> Vec<(usize, usize)> {
    if quote.is_empty() {
        return Vec::new();
    }
    text.char_indices()
        .map(|(start, _)| start)
        .filter(|start| text[*start..].starts_with(quote))
        .map(|start| (start, start + quote.len()))
        .collect()
}

pub(crate) fn snapshots_corpus_equal(left: &Snapshot, right: &Snapshot) -> bool {
    let concepts = |snapshot: &Snapshot| {
        snapshot
            .concepts
            .iter()
            .map(|concept| (concept.id, concept.label.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    let edges = |snapshot: &Snapshot| {
        snapshot
            .edges
            .iter()
            .map(|edge| (edge.parent_id, edge.child_id))
            .collect::<BTreeSet<_>>()
    };
    let evidence = |snapshot: &Snapshot| {
        snapshot
            .evidence
            .iter()
            .map(evidence_key)
            .collect::<BTreeSet<_>>()
    };
    concepts(left) == concepts(right)
        && edges(left) == edges(right)
        && evidence(left) == evidence(right)
}

fn evidence_key(evidence: &SnapshotEvidence) -> (i64, i64, usize, usize) {
    (
        evidence.concept_id,
        evidence.work_id,
        evidence.start_byte,
        evidence.end_byte,
    )
}

fn reject_retired(retired: &BTreeSet<i64>, concept: i64) -> Result<(), AppError> {
    if retired.contains(&concept) {
        Err(invalid_change(
            "a retired concept cannot also be changed or used by a parent edge",
        ))
    } else {
        Ok(())
    }
}

fn concept_ref(snapshot: &Snapshot, id: i64) -> Result<&SnapshotConcept, AppError> {
    snapshot
        .concepts
        .iter()
        .find(|concept| concept.id == id)
        .ok_or_else(|| invalid_change(format!("{} is not in the projected corpus", cid_text(id))))
}

fn concept_mut(snapshot: &mut Snapshot, id: i64) -> Result<&mut SnapshotConcept, AppError> {
    snapshot
        .concepts
        .iter_mut()
        .find(|concept| concept.id == id)
        .ok_or_else(|| invalid_change(format!("{} is not in the projected corpus", cid_text(id))))
}

fn concept_reference(snapshot: &Snapshot, id: i64) -> Result<ConceptReference, AppError> {
    let concept = concept_ref(snapshot, id)?;
    Ok(ConceptReference {
        id: cid(id)?,
        label: concept.label.clone(),
    })
}

fn sort_references(references: &mut [ConceptReference]) {
    references.sort_by(|left, right| {
        (index::normalize(&left.label), left.id.storage_id())
            .cmp(&(index::normalize(&right.label), right.id.storage_id()))
    });
}

fn cid(id: i64) -> Result<ConceptId, AppError> {
    ConceptId::from_storage(id).map_err(|_| {
        AppError::database(
            "invalid_concept_id",
            format!("stored concept ID {id} is invalid"),
        )
    })
}

fn cid_text(id: i64) -> String {
    ConceptId::from_storage(id).map_or_else(|_| format!("concept {id}"), |value| value.to_string())
}

fn invalid_change(message: impl Into<String>) -> AppError {
    AppError::invalid("invalid_reconciliation", message)
}

fn contract_error(error: &ReconciliationContractError) -> AppError {
    AppError::invalid("invalid_reconciliation", error.to_string())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use serde_json::json;

    use super::{
        MAX_EVIDENCE_MATCHES, ResolvedOperation, apply_record, matching_quote_ranges,
        replay_record, resolve_quote, submit_value, validate_record,
    };
    use crate::change::EvidenceSelector;
    use crate::corpus::{head_snapshot, store_work};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn quote_context_filters_matches_in_source_order() {
        let work = crate::corpus::Work {
            id: 1,
            label: "Paper".to_owned(),
            text: "# One\nLocks work.\n# Two\nLocks work.".to_owned(),
            sha256: String::new(),
            created_at: String::new(),
        };
        let ambiguous = EvidenceSelector {
            quote: "Locks work.".to_owned(),
            within_heading: None,
            preceded_by: None,
            followed_by: None,
        };
        let ranges = matching_quote_ranges(&work, &ambiguous);
        assert_eq!(ranges.len(), 2);
        assert!(ranges[0].0 < ranges[1].0);
        assert_eq!(resolve_quote(&work, &ambiguous).unwrap_or_default(), ranges);
        let selected = EvidenceSelector {
            within_heading: Some(vec!["Two".to_owned()]),
            ..ambiguous
        };
        let selected_ranges = resolve_quote(&work, &selected).unwrap_or_default();
        let [(start, end)] = selected_ranges.as_slice() else {
            panic!("heading context did not select one occurrence");
        };
        assert_eq!(&work.text[*start..*end], "Locks work.");
    }

    #[test]
    fn quote_resolution_includes_overlapping_occurrences() {
        let work = crate::corpus::Work {
            id: 1,
            label: "Overlapping".to_owned(),
            text: "the the the".to_owned(),
            sha256: String::new(),
            created_at: String::new(),
        };
        let selector = EvidenceSelector {
            quote: "the the".to_owned(),
            within_heading: None,
            preceded_by: None,
            followed_by: None,
        };
        assert_eq!(
            resolve_quote(&work, &selector).unwrap_or_default(),
            vec![(0, 7), (4, 11)]
        );
    }

    #[test]
    fn quote_resolution_rejects_more_than_the_match_limit() {
        let selector = EvidenceSelector {
            quote: "same".to_owned(),
            within_heading: None,
            preceded_by: None,
            followed_by: None,
        };
        let at_limit = crate::corpus::Work {
            id: 1,
            label: "At limit".to_owned(),
            text: std::iter::repeat_n("same", MAX_EVIDENCE_MATCHES)
                .collect::<Vec<_>>()
                .join(" "),
            sha256: String::new(),
            created_at: String::new(),
        };
        assert_eq!(
            resolve_quote(&at_limit, &selector)
                .unwrap_or_default()
                .len(),
            MAX_EVIDENCE_MATCHES
        );

        let over_limit = crate::corpus::Work {
            id: 1,
            label: "Over limit".to_owned(),
            text: std::iter::repeat_n("same", MAX_EVIDENCE_MATCHES + 1)
                .collect::<Vec<_>>()
                .join(" "),
            sha256: String::new(),
            created_at: String::new(),
        };
        let Err(error) = resolve_quote(&over_limit, &selector) else {
            panic!("match limit was ignored");
        };
        assert_eq!(error.code(), "quote_too_many_matches");

        let absent = EvidenceSelector {
            quote: "absent".to_owned(),
            ..selector
        };
        let Err(error) = resolve_quote(&at_limit, &absent) else {
            panic!("missing quote was accepted");
        };
        assert_eq!(error.code(), "quote_not_found");
    }

    #[test]
    fn repeated_quote_expands_to_range_links_and_one_resolved_evidence_item() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Paper", "Repeated. Repeated.")?;
        let record = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Ground one concept with repeated source text",
                "operations": [{
                    "action":"create_concept",
                    "ref":"repeated",
                    "label":"Repeated",
                    "parents":[],
                    "evidence":[{"quote":"Repeated."}]
                }]
            }),
            "human",
            None,
        )?;
        let resolved = replay_record(&connection, &record)?;
        assert_eq!(resolved.resulting_snapshot.evidence.len(), 2);
        let [ResolvedOperation::CreateConcept { evidence, .. }] = resolved.operations.as_slice()
        else {
            return Err("create receipt missing".into());
        };
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].quote, "Repeated.");
        assert_eq!(evidence[0].occurrence_count, 2);

        apply_record(&mut connection, &record)?;
        assert_eq!(head_snapshot(&connection)?.evidence.len(), 2);
        Ok(())
    }

    #[test]
    fn removing_a_repeated_quote_requires_every_matched_range() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(
            &mut connection,
            "Paper",
            "First: Repeated. Second: Repeated. Grounding.",
        )?;
        let setup = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Attach one repeated occurrence",
                "operations": [{
                    "action":"create_concept",
                    "ref":"repeated",
                    "label":"Repeated",
                    "parents":[],
                    "evidence":[
                        {"quote":"Repeated.","preceded_by":"First: "},
                        {"quote":"Grounding."}
                    ]
                }]
            }),
            "human",
            None,
        )?;
        apply_record(&mut connection, &setup)?;
        let before = head_snapshot(&connection)?;
        assert_eq!(before.evidence.len(), 2);

        let removal = submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Remove every repeated occurrence",
                "operations": [{
                    "action":"remove_evidence",
                    "concept":{"id":"c1"},
                    "evidence":[{"quote":"Repeated."}]
                }]
            }),
            "human",
            None,
        );
        let Err(error) = removal else {
            return Err("partially attached match set was removed".into());
        };
        assert_eq!(error.code(), "evidence_not_attached");
        assert_eq!(head_snapshot(&connection)?, before);

        let attach_all = submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Attach every repeated occurrence",
                "operations": [{
                    "action":"add_evidence",
                    "concept":{"id":"c1"},
                    "evidence":[{"quote":"Repeated."}]
                }]
            }),
            "human",
            None,
        )?;
        apply_record(&mut connection, &attach_all)?;
        assert_eq!(head_snapshot(&connection)?.evidence.len(), 3);

        let remove_all = submit_value(
            &mut connection,
            &work,
            2,
            json!({
                "summary": "Remove every attached repeated occurrence",
                "operations": [{
                    "action":"remove_evidence",
                    "concept":{"id":"c1"},
                    "evidence":[{"quote":"Repeated."}]
                }]
            }),
            "human",
            None,
        )?;
        let resolved = validate_record(&connection, &remove_all)?;
        let [ResolvedOperation::RemoveEvidence { evidence, .. }] = resolved.operations.as_slice()
        else {
            return Err("remove-evidence receipt missing".into());
        };
        assert_eq!(evidence[0].occurrence_count, 2);
        apply_record(&mut connection, &remove_all)?;
        assert_eq!(head_snapshot(&connection)?.evidence.len(), 1);
        Ok(())
    }

    #[test]
    fn creates_and_applies_a_shared_diamond() -> TestResult {
        let mut connection = test_connection()?;
        let text = "A. B. Shared. Leaf.";
        let work = store_work(&mut connection, "Paper", text)?;
        let record = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Create a diamond",
                "operations": [
                    {"action":"create_concept","ref":"a","label":"A","parents":[],"evidence":[{"quote":"A."}]},
                    {"action":"create_concept","ref":"b","label":"B","parents":[],"evidence":[{"quote":"B."}]},
                    {"action":"create_concept","ref":"x","label":"Shared","parents":[{"new":"a"},{"new":"b"}],"evidence":[{"quote":"Shared."}]},
                    {"action":"create_concept","ref":"y","label":"Leaf","parents":[{"new":"x"}],"evidence":[{"quote":"Leaf."}]}
                ]
            }),
            "human",
            None,
        )?;
        assert_eq!(record.status, "pending");
        assert_eq!(apply_record(&mut connection, &record)?, 1);
        let snapshot = head_snapshot(&connection)?;
        assert_eq!(snapshot.concepts.len(), 4);
        assert_eq!(snapshot.edges.len(), 3);
        let shared = snapshot
            .concepts
            .iter()
            .find(|concept| concept.label == "Shared")
            .ok_or("shared concept missing")?;
        assert_eq!(
            snapshot
                .edges
                .iter()
                .filter(|edge| edge.child_id == shared.id)
                .count(),
            2
        );
        Ok(())
    }

    #[test]
    fn rejects_a_cycle_in_the_final_edge_set() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Paper", "A. B.")?;
        let result = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Cycle",
                "operations": [
                    {"action":"create_concept","ref":"a","label":"A","parents":[{"new":"b"}],"evidence":[{"quote":"A."}]},
                    {"action":"create_concept","ref":"b","label":"B","parents":[{"new":"a"}],"evidence":[{"quote":"B."}]}
                ]
            }),
            "human",
            None,
        );
        let Err(error) = result else {
            panic!("cycle was accepted");
        };
        assert_eq!(error.code(), "would_create_cycle");
        assert!(head_snapshot(&connection)?.concepts.is_empty());
        Ok(())
    }

    #[test]
    fn replay_uses_recorded_created_ids_after_sequence_advances() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Paper", "First. Second.")?;
        let record = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Create the first concept",
                "operations": [{
                    "action":"create_concept",
                    "ref":"first",
                    "label":"First",
                    "parents":[],
                    "evidence":[{"quote":"First."}]
                }]
            }),
            "human",
            None,
        )?;
        let expected = replay_record(&connection, &record)?;

        connection.execute("INSERT INTO concept_identities DEFAULT VALUES", [])?;

        assert_eq!(replay_record(&connection, &record)?, expected);
        assert_eq!(apply_record(&mut connection, &record)?, 1);
        assert_eq!(head_snapshot(&connection)?.concepts[0].id, 1);
        Ok(())
    }

    #[test]
    fn duplicate_create_parents_are_rejected() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Paper", "Parent. Child.")?;
        let result = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Duplicate parent",
                "operations": [
                    {"action":"create_concept","ref":"p","label":"Parent","parents":[],"evidence":[{"quote":"Parent."}]},
                    {"action":"create_concept","ref":"c","label":"Child","parents":[{"new":"p"},{"new":"p"}],"evidence":[{"quote":"Child."}]}
                ]
            }),
            "human",
            None,
        );
        let Err(error) = result else {
            panic!("duplicate parent was accepted");
        };
        assert_eq!(error.code(), "invalid_reconciliation");
        Ok(())
    }

    #[test]
    fn adding_an_existing_parent_is_an_idempotent_ensure() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Paper", "Parent. Child.")?;
        let setup = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Create an edge",
                "operations": [
                    {"action":"create_concept","ref":"p","label":"Parent","parents":[],"evidence":[{"quote":"Parent."}]},
                    {"action":"create_concept","ref":"c","label":"Child","parents":[{"new":"p"}],"evidence":[{"quote":"Child."}]}
                ]
            }),
            "human",
            None,
        )?;
        apply_record(&mut connection, &setup)?;

        let repeated = submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Ensure the existing edge twice",
                "operations": [
                    {"action":"add_parent","concept":{"id":"c2"},"parent":{"id":"c1"}},
                    {"action":"add_parent","concept":{"id":"c2"},"parent":{"id":"c1"}}
                ]
            }),
            "human",
            None,
        )?;
        assert_eq!(repeated.status, "recorded");
        assert_eq!(head_snapshot(&connection)?.edges.len(), 1);

        let contradictory = submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Contradict the edge",
                "operations": [
                    {"action":"add_parent","concept":{"id":"c2"},"parent":{"id":"c1"}},
                    {"action":"remove_parent","concept":{"id":"c2"},"parent":{"id":"c1"}}
                ]
            }),
            "human",
            None,
        );
        let Err(error) = contradictory else {
            return Err("the same parent edge was added and removed".into());
        };
        assert_eq!(error.code(), "invalid_reconciliation");
        Ok(())
    }

    #[test]
    fn retirement_receipt_records_all_incident_edges() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Paper", "Parent. Retired. Child.")?;
        let setup = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Create a chain",
                "operations": [
                    {"action":"create_concept","ref":"p","label":"Parent","parents":[],"evidence":[{"quote":"Parent."}]},
                    {"action":"create_concept","ref":"x","label":"Retired","parents":[{"new":"p"}],"evidence":[{"quote":"Retired."}]},
                    {"action":"create_concept","ref":"c","label":"Child","parents":[{"new":"x"}],"evidence":[{"quote":"Child."}]}
                ]
            }),
            "human",
            None,
        )?;
        apply_record(&mut connection, &setup)?;
        let retire = submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Retire the middle concept",
                "operations": [{
                    "action":"retire_concept",
                    "concept":{"id":"c2"}
                }]
            }),
            "human",
            None,
        )?;
        let resolved = validate_record(&connection, &retire)?;
        let [
            ResolvedOperation::RetireConcept {
                removed_parents,
                removed_children,
                ..
            },
        ] = resolved.operations.as_slice()
        else {
            return Err("retirement receipt missing".into());
        };
        assert_eq!(removed_parents[0].id.to_string(), "c1");
        assert_eq!(removed_children[0].id.to_string(), "c3");
        Ok(())
    }

    #[test]
    fn retirement_receipt_can_reference_a_new_replacement() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Paper", "Old. New.")?;
        let setup = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Create the old concept",
                "operations": [{
                    "action":"create_concept",
                    "ref":"old",
                    "label":"Old",
                    "parents":[],
                    "evidence":[{"quote":"Old."}]
                }]
            }),
            "human",
            None,
        )?;
        apply_record(&mut connection, &setup)?;

        let retire = submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Replace the old concept",
                "operations": [
                    {"action":"create_concept","ref":"new","label":"New","parents":[],"evidence":[{"quote":"New."}]},
                    {"action":"retire_concept","concept":{"id":"c1"},"replacement":{"new":"new"}}
                ]
            }),
            "human",
            None,
        )?;
        let resolved = validate_record(&connection, &retire)?;
        let ResolvedOperation::RetireConcept {
            replacement: Some(replacement),
            ..
        } = &resolved.operations[1]
        else {
            return Err("retirement replacement receipt missing".into());
        };
        assert_eq!(replacement.id.to_string(), "c2");
        assert_eq!(replacement.label, "New");
        assert_eq!(apply_record(&mut connection, &retire)?, 2);
        Ok(())
    }

    fn test_connection() -> Result<Connection, Box<dyn std::error::Error>> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(include_str!("../schema.sql"))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        Ok(connection)
    }
}
