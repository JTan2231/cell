use std::collections::{BTreeSet, HashMap};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::change::{
    ChangeOperation, ConceptSelector, EvidenceDisposition, EvidenceSelector, Reconciliation,
    ReconciliationContractError, parse_reconciliation, parse_reconciliation_value,
};
use crate::corpus::{
    ReconciliationRecord, Snapshot, SnapshotConcept, SnapshotEvidence, Work, head_snapshot,
    heading_for_offset, insert_commit, insert_reconciliation, materialize_snapshot, next_position,
    path_lookup, paths, renumber_siblings, revision, sequence_next, snapshot_at, validate_snapshot,
};
use crate::error::AppError;
use crate::index;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolvedReconciliation {
    pub base_revision: i64,
    pub operations: Vec<ResolvedOperation>,
    pub resulting_snapshot: Snapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ResolvedOperation {
    CreateConcept {
        path: Vec<String>,
        evidence_quotes: Vec<String>,
    },
    AddEvidence {
        path: Vec<String>,
        quotes: Vec<String>,
    },
    RemoveEvidence {
        path: Vec<String>,
        quotes: Vec<String>,
    },
    MoveConcept {
        before: Vec<String>,
        after: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_sibling_before: Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_sibling_after: Option<Vec<String>>,
    },
    RewordConcept {
        before: Vec<String>,
        after: Vec<String>,
        evidence_disposition: EvidenceDisposition,
    },
    RetireConcept {
        path: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        replacement: Option<Vec<String>>,
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
    let request: Value = serde_json::from_str(document)?;
    submit(
        connection,
        work,
        base_revision,
        &request,
        &reconciliation,
        actor,
        model_run_id,
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
        parse_reconciliation_value(value.clone()).map_err(|error| contract_error(&error))?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let record = submit_parsed(
        &transaction,
        work,
        base_revision,
        &value,
        &reconciliation,
        actor,
        model_run_id,
    )?;
    transaction.commit()?;
    Ok(record)
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn submit_value_in_transaction(
    transaction: &Transaction<'_>,
    work: &Work,
    base_revision: i64,
    value: Value,
    actor: &str,
    model_run_id: Option<i64>,
) -> Result<ReconciliationRecord, AppError> {
    let reconciliation =
        parse_reconciliation_value(value.clone()).map_err(|error| contract_error(&error))?;
    submit_parsed(
        transaction,
        work,
        base_revision,
        &value,
        &reconciliation,
        actor,
        model_run_id,
    )
}

fn submit(
    connection: &mut Connection,
    work: &Work,
    base_revision: i64,
    request: &Value,
    reconciliation: &Reconciliation,
    actor: &str,
    model_run_id: Option<i64>,
) -> Result<ReconciliationRecord, AppError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let record = submit_parsed(
        &transaction,
        work,
        base_revision,
        request,
        reconciliation,
        actor,
        model_run_id,
    )?;
    transaction.commit()?;
    Ok(record)
}

fn submit_parsed(
    connection: &Transaction<'_>,
    work: &Work,
    base_revision: i64,
    request: &Value,
    reconciliation: &Reconciliation,
    actor: &str,
    model_run_id: Option<i64>,
) -> Result<ReconciliationRecord, AppError> {
    let base = snapshot_at(connection, base_revision)?;
    let resolution_timestamp = crate::corpus::now()?;
    let resolved = resolve(
        connection,
        work,
        base_revision,
        &base,
        reconciliation,
        &resolution_timestamp,
    )?;
    let changes_corpus = !snapshots_corpus_equal(&base, &resolved.resulting_snapshot);
    let resolved_json = serde_json::to_string(&resolved)?;
    let request_json = serde_json::to_string(request)?;
    insert_reconciliation(
        connection,
        work.id,
        base_revision,
        model_run_id,
        changes_corpus,
        reconciliation.summary(),
        &request_json,
        &resolved_json,
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
    let expected: ResolvedReconciliation = serde_json::from_str(&record.resolved_reconciliation)?;
    let base = snapshot_at(connection, record.base_revision)?;
    if snapshots_corpus_equal(&base, &expected.resulting_snapshot) {
        return Err(AppError::database(
            "invalid_resolved_reconciliation",
            "a pending reconciliation must project a corpus transition",
        ));
    }
    validate_snapshot(connection, &expected.resulting_snapshot).map_err(|error| {
        AppError::database(
            "invalid_resolved_reconciliation",
            format!("the stored resulting corpus is invalid: {error}"),
        )
    })?;
    let actual = replay_record(connection, record)?;
    if actual != expected {
        return Err(AppError::database(
            "invalid_resolved_reconciliation",
            "the stored resolved reconciliation does not match its submitted request",
        ));
    }
    Ok(expected)
}

pub(crate) fn replay_record(
    connection: &Connection,
    record: &ReconciliationRecord,
) -> Result<ResolvedReconciliation, AppError> {
    let expected: ResolvedReconciliation = serde_json::from_str(&record.resolved_reconciliation)
        .map_err(|error| {
            AppError::database(
                "invalid_resolved_reconciliation",
                format!("the stored resolved reconciliation is invalid: {error}"),
            )
        })?;
    if expected.base_revision != record.base_revision {
        return Err(AppError::database(
            "invalid_resolved_reconciliation",
            "the stored resolved reconciliation names a different base revision",
        ));
    }
    let reconciliation =
        parse_reconciliation(&record.submitted_request).map_err(|error| contract_error(&error))?;
    let work = crate::corpus::get_work_by_id(connection, record.work_id)?;
    let base = snapshot_at(connection, record.base_revision)?;
    let timestamp = resolution_timestamp(&base, &expected);
    resolve(
        connection,
        &work,
        record.base_revision,
        &base,
        &reconciliation,
        &timestamp,
    )
}

pub(crate) fn apply_record(
    connection: &mut Connection,
    record: &ReconciliationRecord,
) -> Result<i64, AppError> {
    if record.status != "pending" {
        return Err(AppError::conflict(
            "nothing_to_apply",
            "the selected reconciliation is not pending",
        ));
    }
    let resolved = validate_record(connection, record)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current_status: Option<String> = transaction
        .query_row(
            "SELECT status FROM reconciliations WHERE id = ?1",
            [record.id],
            |row| row.get(0),
        )
        .optional()?;
    match current_status.as_deref() {
        Some("pending") => {}
        Some(_) => {
            return Err(AppError::conflict(
                "nothing_to_apply",
                "the selected reconciliation is no longer pending",
            ));
        }
        None => {
            return Err(AppError::not_found(
                "pending_reconciliation_not_found",
                "the selected reconciliation no longer exists",
            ));
        }
    }
    let head_revision = revision(&transaction)?;
    if head_revision != record.base_revision {
        return Err(stale_change(record.base_revision, head_revision));
    }
    let before = head_snapshot(&transaction)?;
    let reconciliation =
        parse_reconciliation(&record.submitted_request).map_err(|error| contract_error(&error))?;
    let work = crate::corpus::get_work_by_id(&transaction, record.work_id)?;
    let timestamp = resolution_timestamp(&before, &resolved);
    let revalidated = resolve(
        &transaction,
        &work,
        record.base_revision,
        &before,
        &reconciliation,
        &timestamp,
    )?;
    if revalidated != resolved {
        return Err(AppError::conflict(
            "reconciliation_resolution_changed",
            "the stored reconciliation no longer resolves to the same transition",
        ));
    }
    let new_revision = head_revision.checked_add(1).ok_or_else(|| {
        AppError::database("revision_overflow", "the corpus revision is too large")
    })?;
    materialize_snapshot(&transaction, &resolved.resulting_snapshot)?;
    index::rebuild_all(&transaction)?;
    insert_commit(
        &transaction,
        new_revision,
        Some(record.work_id),
        Some(record.id),
        "change",
        &record.summary,
        &serde_json::from_str(&record.submitted_request)?,
        &serde_json::to_value(&resolved.operations)?,
        &before,
        &resolved.resulting_snapshot,
        &json!({ "reconciliation_actor": record.actor }),
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
    transaction.commit()?;
    Ok(new_revision)
}

fn resolution_timestamp(base: &Snapshot, resolved: &ResolvedReconciliation) -> String {
    let base_ids = base
        .evidence
        .iter()
        .map(|evidence| evidence.id)
        .collect::<BTreeSet<_>>();
    resolved
        .resulting_snapshot
        .evidence
        .iter()
        .find(|evidence| !base_ids.contains(&evidence.id))
        .map_or_else(
            || "1970-01-01T00:00:00Z".to_owned(),
            |evidence| evidence.created_at.clone(),
        )
}

fn stale_change(base_revision: i64, head_revision: i64) -> AppError {
    AppError::conflict(
        "stale_change",
        format!(
            "the reconciliation examined revision {base_revision}, but HEAD is revision {head_revision}"
        ),
    )
}

#[allow(clippy::too_many_lines)]
fn resolve(
    connection: &Connection,
    work: &Work,
    base_revision: i64,
    base: &Snapshot,
    reconciliation: &Reconciliation,
    timestamp: &str,
) -> Result<ResolvedReconciliation, AppError> {
    let operations = reconciliation.operations();
    validate_snapshot(connection, base).map_err(|error| {
        AppError::database(
            "invalid_history_snapshot",
            format!("revision {base_revision} contains an invalid corpus: {error}"),
        )
    })?;
    let existing = path_lookup(base)?;
    let mut local_ids = HashMap::new();
    let mut next_concept_id = sequence_next(connection, "concepts")?;
    for operation in operations {
        if let ChangeOperation::CreateConcept { label, .. } = operation {
            local_ids.insert(index::normalize(label), next_concept_id);
            next_concept_id = next_concept_id.checked_add(1).ok_or_else(|| {
                AppError::database("identity_overflow", "concept identity space is exhausted")
            })?;
        }
    }
    let resolve_selector = |selector: &ConceptSelector| -> Result<i64, AppError> {
        match selector {
            ConceptSelector::Existing { path } => existing
                .get(
                    &path
                        .iter()
                        .map(|segment| index::normalize(segment))
                        .collect::<Vec<_>>(),
                )
                .copied()
                .ok_or_else(|| {
                    AppError::not_found(
                        "concept_not_found",
                        format!(
                            "concept path {} was not found at revision {base_revision}",
                            display_path(path)
                        ),
                    )
                }),
            ConceptSelector::New { label } => local_ids
                .get(&index::normalize(label))
                .copied()
                .ok_or_else(|| invalid_change(format!("new concept {label:?} was not declared"))),
        }
    };
    let resolved_selectors = operations
        .iter()
        .map(|operation| resolve_operation_selectors(operation, &resolve_selector))
        .collect::<Result<Vec<_>, AppError>>()?;

    let new_revision = base_revision.checked_add(1).ok_or_else(|| {
        AppError::database("revision_overflow", "the corpus revision is too large")
    })?;
    let mut result = base.clone();
    for (operation, selectors) in operations.iter().zip(&resolved_selectors) {
        if let (
            ChangeOperation::CreateConcept { label, .. },
            ResolvedSelectors::Create { concept, under, .. },
        ) = (operation, selectors)
        {
            let position = next_position(&result, *under)?;
            result.concepts.push(SnapshotConcept {
                id: *concept,
                parent_id: *under,
                label: label.clone(),
                position,
                created_revision: new_revision,
                updated_revision: new_revision,
            });
        }
    }
    let initial_paths = paths(&result).map_err(|_| {
        AppError::conflict(
            "would_create_cycle",
            "the proposed concept creations contain a parent cycle",
        )
    })?;
    let original_parents = result
        .concepts
        .iter()
        .map(|concept| (concept.id, concept.parent_id))
        .collect::<HashMap<_, _>>();

    let mut retired = BTreeSet::new();
    let mut moved = BTreeSet::new();
    let mut reworded = BTreeSet::new();
    for (operation, selectors) in operations.iter().zip(&resolved_selectors) {
        match (operation, selectors) {
            (
                ChangeOperation::RetireConcept { .. },
                ResolvedSelectors::Retire {
                    concept,
                    replacement,
                },
            ) => {
                if !retired.insert(*concept) {
                    return Err(invalid_change(
                        "a concept cannot be retired more than once in one request",
                    ));
                }
                if replacement == &Some(*concept) {
                    return Err(invalid_change("a retired concept cannot replace itself"));
                }
            }
            (ChangeOperation::MoveConcept { .. }, ResolvedSelectors::Move { concept, .. }) => {
                if !moved.insert(*concept) {
                    return Err(invalid_change(
                        "a concept cannot be moved more than once in one request",
                    ));
                }
            }
            (ChangeOperation::RewordConcept { .. }, ResolvedSelectors::One { concept })
                if !reworded.insert(*concept) =>
            {
                return Err(invalid_change(
                    "a concept cannot be reworded more than once in one request",
                ));
            }
            _ => {}
        }
    }
    for selectors in &resolved_selectors {
        match selectors {
            ResolvedSelectors::Create {
                concept,
                under,
                before,
                after,
            }
            | ResolvedSelectors::Move {
                concept,
                under,
                before,
                after,
            } => {
                reject_retired_target(&retired, *concept)?;
                if under.is_some_and(|id| retired.contains(&id)) {
                    return Err(invalid_change(
                        "a destination parent must survive the complete request",
                    ));
                }
                reject_retired_anchor(&retired, *before, *after)?;
            }
            ResolvedSelectors::One { concept } => reject_retired_target(&retired, *concept)?,
            ResolvedSelectors::Retire { replacement, .. } => {
                if replacement.as_ref().is_some_and(|id| retired.contains(id)) {
                    return Err(invalid_change(
                        "a retirement replacement must survive the complete request",
                    ));
                }
            }
        }
    }

    // Parent assignments form one final topology. Applying them before ordering makes
    // coherent swaps independent of the request's operation order.
    for selectors in &resolved_selectors {
        if let ResolvedSelectors::Move { concept, under, .. } = selectors {
            concept_mut(&mut result, *concept)?.parent_id = *under;
        }
    }
    paths(&result).map_err(|_| {
        AppError::conflict(
            "would_create_cycle",
            "the proposed final parent relationships would create a cycle",
        )
    })?;

    let mut next_evidence_id = sequence_next(connection, "evidence")?;
    let base_evidence_ids = base
        .evidence
        .iter()
        .map(|evidence| evidence.id)
        .collect::<BTreeSet<_>>();
    let mut receipts = vec![None; operations.len()];
    for (operation_index, (operation, selectors)) in
        operations.iter().zip(&resolved_selectors).enumerate()
    {
        match (operation, selectors) {
            (
                ChangeOperation::CreateConcept {
                    label, evidence, ..
                },
                ResolvedSelectors::Create {
                    concept,
                    under,
                    before,
                    after,
                },
            ) => {
                let position =
                    placement_position(&mut result, *under, *before, *after, Some(*concept))?;
                concept_mut(&mut result, *concept)?.position = position;
                let quotes = add_evidence(
                    &mut result,
                    *concept,
                    work,
                    evidence,
                    timestamp,
                    &mut next_evidence_id,
                )?;
                receipts[operation_index] = Some(ResolvedOperation::CreateConcept {
                    path: initial_paths.get(concept).cloned().ok_or_else(|| {
                        AppError::unexpected(
                            "created_path_missing",
                            format!("created concept {label:?} has no projected path"),
                        )
                    })?,
                    evidence_quotes: quotes,
                });
            }
            (ChangeOperation::AddEvidence { evidence, .. }, ResolvedSelectors::One { concept }) => {
                let path = concept_path(&result, *concept)?;
                let quotes = add_evidence(
                    &mut result,
                    *concept,
                    work,
                    evidence,
                    timestamp,
                    &mut next_evidence_id,
                )?;
                receipts[operation_index] = Some(ResolvedOperation::AddEvidence { path, quotes });
            }
            (
                ChangeOperation::RemoveEvidence { evidence, .. },
                ResolvedSelectors::One { concept },
            ) => {
                let path = concept_path(&result, *concept)?;
                let mut ranges = Vec::new();
                let mut quotes = Vec::new();
                for selector in evidence {
                    let (start, end) = resolve_quote(work, selector)?;
                    ranges.push((start, end));
                    quotes.push(selector.quote.clone());
                }
                for (start, end) in ranges {
                    let before = result.evidence.len();
                    result.evidence.retain(|item| {
                        !(item.concept_id == *concept
                            && item.work_id == work.id
                            && item.start_byte == start
                            && item.end_byte == end)
                    });
                    if result.evidence.len() == before {
                        return Err(AppError::not_found(
                            "evidence_not_found",
                            format!(
                                "the selected quotation is not attached to {}",
                                display_path(&path)
                            ),
                        ));
                    }
                }
                receipts[operation_index] =
                    Some(ResolvedOperation::RemoveEvidence { path, quotes });
            }
            (
                ChangeOperation::MoveConcept { .. },
                ResolvedSelectors::Move {
                    concept,
                    under,
                    before,
                    after,
                },
            ) => {
                let old_path = initial_paths.get(concept).cloned().ok_or_else(|| {
                    invalid_change(
                        "a moved concept has no path in the reconciliation's initial corpus",
                    )
                })?;
                let old_previous_sibling = previous_sibling_path(base, *concept)?;
                let old_parent = original_parents.get(concept).copied().flatten();
                let position =
                    placement_position(&mut result, *under, *before, *after, Some(*concept))?;
                let item = concept_mut(&mut result, *concept)?;
                item.position = position;
                renumber_siblings(&mut result, old_parent);
                renumber_siblings(&mut result, *under);
                let new_path = concept_path(&result, *concept)?;
                let new_previous_sibling = previous_sibling_path(&result, *concept)?;
                receipts[operation_index] = Some(ResolvedOperation::MoveConcept {
                    before: old_path,
                    after: new_path,
                    previous_sibling_before: old_previous_sibling,
                    previous_sibling_after: new_previous_sibling,
                });
            }
            (
                ChangeOperation::RewordConcept {
                    label,
                    evidence_disposition,
                    ..
                },
                ResolvedSelectors::One { concept },
            ) => {
                let old_path = concept_path(&result, *concept)?;
                let item = concept_mut(&mut result, *concept)?;
                item.label.clone_from(label);
                if *evidence_disposition == EvidenceDisposition::Remove {
                    result.evidence.retain(|item| {
                        item.concept_id != *concept || !base_evidence_ids.contains(&item.id)
                    });
                }
                let new_path = concept_path(&result, *concept)?;
                receipts[operation_index] = Some(ResolvedOperation::RewordConcept {
                    before: old_path,
                    after: new_path,
                    evidence_disposition: *evidence_disposition,
                });
            }
            (ChangeOperation::RetireConcept { .. }, ResolvedSelectors::Retire { .. }) => {}
            _ => {
                return Err(AppError::unexpected(
                    "resolver_mismatch",
                    "validated change operation did not match its resolved selectors",
                ));
            }
        }
    }
    apply_ordering_constraints(&mut result, &resolved_selectors)?;
    for (operation_index, selector) in resolved_selectors.iter().enumerate() {
        if let ResolvedSelectors::Move { concept, .. } = selector
            && let Some(ResolvedOperation::MoveConcept {
                previous_sibling_after,
                ..
            }) = &mut receipts[operation_index]
        {
            *previous_sibling_after = previous_sibling_path(&result, *concept)?;
        }
    }

    let before_retirement_paths = paths(&result)?;
    for concept in &retired {
        let path = before_retirement_paths
            .get(concept)
            .cloned()
            .unwrap_or_default();
        if result
            .concepts
            .iter()
            .any(|item| item.parent_id == Some(*concept) && !retired.contains(&item.id))
        {
            return Err(AppError::conflict(
                "retire_has_children",
                format!(
                    "{} still has children; move or retire each child explicitly",
                    display_path(&path)
                ),
            ));
        }
    }
    for (operation_index, selectors) in resolved_selectors.iter().enumerate() {
        if let ResolvedSelectors::Retire {
            concept,
            replacement,
        } = selectors
        {
            let path = before_retirement_paths
                .get(concept)
                .cloned()
                .ok_or_else(|| invalid_change("a retired concept has no projected path"))?;
            receipts[operation_index] = Some(ResolvedOperation::RetireConcept {
                path,
                replacement: replacement
                    .map(|id| {
                        before_retirement_paths.get(&id).cloned().ok_or_else(|| {
                            invalid_change("a retirement replacement has no projected path")
                        })
                    })
                    .transpose()?,
            });
        }
    }
    let affected_parents = result
        .concepts
        .iter()
        .filter(|concept| retired.contains(&concept.id))
        .filter_map(|concept| concept.parent_id)
        .filter(|parent| !retired.contains(parent))
        .map(Some)
        .chain(
            result
                .concepts
                .iter()
                .any(|concept| retired.contains(&concept.id) && concept.parent_id.is_none())
                .then_some(None),
        )
        .collect::<BTreeSet<_>>();
    result
        .concepts
        .retain(|concept| !retired.contains(&concept.id));
    result
        .evidence
        .retain(|evidence| !retired.contains(&evidence.concept_id));
    for parent in affected_parents {
        renumber_siblings(&mut result, parent);
    }
    let unchanged = snapshots_corpus_equal(base, &result);
    let base_concepts = base
        .concepts
        .iter()
        .map(|concept| (concept.id, concept))
        .collect::<HashMap<_, _>>();
    if unchanged {
        result = base.clone();
    } else {
        for concept in &mut result.concepts {
            if base_concepts.get(&concept.id).is_some_and(|base_concept| {
                concept.parent_id != base_concept.parent_id
                    || concept.label != base_concept.label
                    || concept.position != base_concept.position
            }) {
                concept.updated_revision = new_revision;
            }
        }
    }
    validate_snapshot(connection, &result)?;
    let receipts = receipts
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| {
            AppError::unexpected(
                "resolver_receipt_missing",
                "a validated operation has no resolved receipt",
            )
        })?;
    Ok(ResolvedReconciliation {
        base_revision,
        operations: receipts,
        resulting_snapshot: result,
    })
}

#[derive(Clone, Copy)]
enum ResolvedSelectors {
    Create {
        concept: i64,
        under: Option<i64>,
        before: Option<i64>,
        after: Option<i64>,
    },
    One {
        concept: i64,
    },
    Move {
        concept: i64,
        under: Option<i64>,
        before: Option<i64>,
        after: Option<i64>,
    },
    Retire {
        concept: i64,
        replacement: Option<i64>,
    },
}

fn resolve_operation_selectors(
    operation: &ChangeOperation,
    resolve: &impl Fn(&ConceptSelector) -> Result<i64, AppError>,
) -> Result<ResolvedSelectors, AppError> {
    match operation {
        ChangeOperation::CreateConcept {
            label,
            under,
            before,
            after,
            ..
        } => Ok(ResolvedSelectors::Create {
            concept: resolve(&ConceptSelector::New {
                label: label.clone(),
            })?,
            under: under.as_ref().map(resolve).transpose()?,
            before: before.as_ref().map(resolve).transpose()?,
            after: after.as_ref().map(resolve).transpose()?,
        }),
        ChangeOperation::AddEvidence { concept, .. }
        | ChangeOperation::RemoveEvidence { concept, .. }
        | ChangeOperation::RewordConcept { concept, .. } => Ok(ResolvedSelectors::One {
            concept: resolve(concept)?,
        }),
        ChangeOperation::MoveConcept {
            concept,
            under,
            before,
            after,
        } => Ok(ResolvedSelectors::Move {
            concept: resolve(concept)?,
            under: under.as_ref().map(resolve).transpose()?,
            before: before.as_ref().map(resolve).transpose()?,
            after: after.as_ref().map(resolve).transpose()?,
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

fn reject_retired_target(retired: &BTreeSet<i64>, concept: i64) -> Result<(), AppError> {
    if retired.contains(&concept) {
        Err(invalid_change(
            "a retired concept cannot also be created, moved, reworded, or have evidence changed",
        ))
    } else {
        Ok(())
    }
}

fn reject_retired_anchor(
    retired: &BTreeSet<i64>,
    before: Option<i64>,
    after: Option<i64>,
) -> Result<(), AppError> {
    if before.or(after).is_some_and(|id| retired.contains(&id)) {
        Err(invalid_change(
            "a before/after ordering anchor must survive the complete request",
        ))
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_lines)]
fn apply_ordering_constraints(
    snapshot: &mut Snapshot,
    selectors: &[ResolvedSelectors],
) -> Result<(), AppError> {
    let mut edges = HashMap::<Option<i64>, BTreeSet<(i64, i64)>>::new();
    for selector in selectors {
        let (concept, before, after) = match *selector {
            ResolvedSelectors::Create {
                concept,
                before,
                after,
                ..
            }
            | ResolvedSelectors::Move {
                concept,
                before,
                after,
                ..
            } => (concept, before, after),
            ResolvedSelectors::One { .. } | ResolvedSelectors::Retire { .. } => continue,
        };
        let item = concept_ref(snapshot, concept)?;
        if let Some(anchor) = before {
            let anchor = concept_ref(snapshot, anchor)?;
            if item.parent_id != anchor.parent_id {
                return Err(AppError::conflict(
                    "invalid_placement",
                    "an ordering anchor is not a child of the selected destination",
                ));
            }
            edges
                .entry(item.parent_id)
                .or_default()
                .insert((concept, anchor.id));
        }
        if let Some(anchor) = after {
            let anchor = concept_ref(snapshot, anchor)?;
            if item.parent_id != anchor.parent_id {
                return Err(AppError::conflict(
                    "invalid_placement",
                    "an ordering anchor is not a child of the selected destination",
                ));
            }
            edges
                .entry(item.parent_id)
                .or_default()
                .insert((anchor.id, concept));
        }
    }
    for (parent, constraints) in edges {
        let mut siblings = snapshot
            .concepts
            .iter()
            .filter(|concept| concept.parent_id == parent)
            .map(|concept| concept.id)
            .collect::<Vec<_>>();
        siblings.sort_by_key(|id| {
            snapshot
                .concepts
                .iter()
                .find(|concept| concept.id == *id)
                .map_or((i64::MAX, *id), |concept| (concept.position, concept.id))
        });
        let mut indegree = siblings
            .iter()
            .map(|id| (*id, 0_usize))
            .collect::<HashMap<_, _>>();
        let mut outgoing = HashMap::<i64, Vec<i64>>::new();
        for (before, after) in constraints {
            let degree = indegree.get_mut(&after).ok_or_else(|| {
                AppError::conflict(
                    "invalid_placement",
                    "an ordering constraint names a concept outside its sibling group",
                )
            })?;
            *degree = degree.checked_add(1).ok_or_else(|| {
                AppError::database("ordering_overflow", "too many ordering constraints")
            })?;
            outgoing.entry(before).or_default().push(after);
        }
        let mut ordered = Vec::with_capacity(siblings.len());
        while ordered.len() < siblings.len() {
            let next = siblings
                .iter()
                .copied()
                .find(|id| !ordered.contains(id) && indegree.get(id) == Some(&0))
                .ok_or_else(|| {
                    AppError::conflict(
                        "invalid_placement",
                        "the reconciliation contains contradictory before/after ordering constraints",
                    )
                })?;
            ordered.push(next);
            for successor in outgoing.get(&next).into_iter().flatten() {
                let degree = indegree.get_mut(successor).ok_or_else(|| {
                    AppError::unexpected(
                        "ordering_resolution_failed",
                        "an ordering successor is missing from its sibling group",
                    )
                })?;
                *degree = degree.checked_sub(1).ok_or_else(|| {
                    AppError::unexpected(
                        "ordering_resolution_failed",
                        "an ordering constraint was resolved more than once",
                    )
                })?;
            }
        }
        for (index, id) in ordered.into_iter().enumerate() {
            concept_mut(snapshot, id)?.position = i64::try_from(index)
                .map_err(|_| AppError::database("position_overflow", "too many siblings"))?
                .checked_mul(1024)
                .ok_or_else(|| {
                    AppError::database("position_overflow", "concept order is too large")
                })?;
        }
    }
    Ok(())
}

pub(crate) fn snapshots_corpus_equal(left: &Snapshot, right: &Snapshot) -> bool {
    let concepts = |snapshot: &Snapshot| {
        snapshot
            .concepts
            .iter()
            .map(|concept| {
                (
                    concept.id,
                    concept.parent_id,
                    concept.label.clone(),
                    concept.position,
                )
            })
            .collect::<BTreeSet<_>>()
    };
    let evidence = |snapshot: &Snapshot| {
        snapshot
            .evidence
            .iter()
            .map(|item| {
                (
                    item.concept_id,
                    item.work_id,
                    item.start_byte,
                    item.end_byte,
                )
            })
            .collect::<BTreeSet<_>>()
    };
    concepts(left) == concepts(right) && evidence(left) == evidence(right)
}

fn placement_position(
    snapshot: &mut Snapshot,
    parent: Option<i64>,
    before: Option<i64>,
    after: Option<i64>,
    moving: Option<i64>,
) -> Result<i64, AppError> {
    let anchor = before.or(after);
    if let Some(anchor) = anchor {
        let anchor_node = concept_ref(snapshot, anchor)?;
        if anchor_node.parent_id != parent || Some(anchor) == moving {
            return Err(AppError::conflict(
                "invalid_placement",
                "before/after must name a different child of the selected destination",
            ));
        }
    }
    let mut siblings = snapshot
        .concepts
        .iter()
        .filter(|item| item.parent_id == parent && Some(item.id) != moving)
        .map(|item| item.id)
        .collect::<Vec<_>>();
    siblings.sort_by_key(|id| {
        let item = snapshot
            .concepts
            .iter()
            .find(|candidate| candidate.id == *id);
        item.map_or((i64::MAX, *id), |item| (item.position, item.id))
    });
    let destination = if let Some(anchor) = before {
        siblings.iter().position(|id| *id == anchor)
    } else if let Some(anchor) = after {
        siblings
            .iter()
            .position(|id| *id == anchor)
            .map(|index| index + 1)
    } else {
        Some(siblings.len())
    }
    .ok_or_else(|| AppError::conflict("invalid_placement", "ordering anchor was not found"))?;
    if let Some(moving) = moving {
        siblings.insert(destination, moving);
        for (index, id) in siblings.iter().copied().enumerate() {
            if let Ok(item) = concept_mut(snapshot, id) {
                item.position = i64::try_from(index)
                    .map_err(|_| AppError::database("position_overflow", "too many siblings"))?
                    * 1024;
            }
        }
        Ok(concept_ref(snapshot, moving)?.position)
    } else if destination == siblings.len() {
        next_position(snapshot, parent)
    } else {
        for (index, id) in siblings.iter().copied().enumerate() {
            let adjusted = index + usize::from(index >= destination);
            concept_mut(snapshot, id)?.position = i64::try_from(adjusted)
                .map_err(|_| AppError::database("position_overflow", "too many siblings"))?
                * 1024;
        }
        Ok(i64::try_from(destination)
            .map_err(|_| AppError::database("position_overflow", "too many siblings"))?
            * 1024)
    }
}

fn add_evidence(
    snapshot: &mut Snapshot,
    concept_id: i64,
    work: &Work,
    selectors: &[EvidenceSelector],
    timestamp: &str,
    next_id: &mut i64,
) -> Result<Vec<String>, AppError> {
    concept_ref(snapshot, concept_id)?;
    let mut quotes = Vec::new();
    for selector in selectors {
        let (start, end) = resolve_quote(work, selector)?;
        if snapshot.evidence.iter().any(|item| {
            item.concept_id == concept_id
                && item.work_id == work.id
                && item.start_byte == start
                && item.end_byte == end
        }) {
            continue;
        }
        snapshot.evidence.push(SnapshotEvidence {
            id: *next_id,
            concept_id,
            work_id: work.id,
            start_byte: start,
            end_byte: end,
            created_at: timestamp.to_owned(),
        });
        *next_id = next_id.checked_add(1).ok_or_else(|| {
            AppError::database("identity_overflow", "evidence identity space is exhausted")
        })?;
        quotes.push(selector.quote.clone());
    }
    Ok(quotes)
}

pub(crate) fn resolve_quote(
    work: &Work,
    selector: &EvidenceSelector,
) -> Result<(usize, usize), AppError> {
    let mut candidates = work
        .text
        .match_indices(&selector.quote)
        .map(|(start, _)| (start, start + selector.quote.len()))
        .collect::<Vec<_>>();
    if let Some(heading) = &selector.within_heading {
        let normalized = heading
            .iter()
            .map(|segment| index::normalize(segment))
            .collect::<Vec<_>>();
        candidates.retain(|(start, _)| {
            heading_for_offset(&work.text, *start).is_some_and(|path| {
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
    match candidates.as_slice() {
        [(start, end)] => Ok((*start, *end)),
        [] => Err(AppError::not_found(
            "quote_not_found",
            format!(
                "quotation {:?} was not found in work {:?}",
                selector.quote, work.label
            ),
        )),
        _ => Err(AppError::conflict(
            "quote_ambiguous",
            format!(
                "quotation {:?} occurs {} times in work {:?}; add heading or neighboring context",
                selector.quote,
                candidates.len(),
                work.label
            ),
        )),
    }
}

fn concept_ref(snapshot: &Snapshot, id: i64) -> Result<&SnapshotConcept, AppError> {
    snapshot
        .concepts
        .iter()
        .find(|concept| concept.id == id)
        .ok_or_else(|| {
            invalid_change("an operation refers to a concept retired earlier in the request")
        })
}

fn concept_mut(snapshot: &mut Snapshot, id: i64) -> Result<&mut SnapshotConcept, AppError> {
    snapshot
        .concepts
        .iter_mut()
        .find(|concept| concept.id == id)
        .ok_or_else(|| {
            invalid_change("an operation refers to a concept retired earlier in the request")
        })
}

fn concept_path(snapshot: &Snapshot, id: i64) -> Result<Vec<String>, AppError> {
    paths(snapshot)?.get(&id).cloned().ok_or_else(|| {
        invalid_change("an operation refers to a concept that is not in the projected corpus")
    })
}

fn previous_sibling_path(
    snapshot: &Snapshot,
    concept_id: i64,
) -> Result<Option<Vec<String>>, AppError> {
    let concept = concept_ref(snapshot, concept_id)?;
    let mut siblings = snapshot
        .concepts
        .iter()
        .filter(|candidate| candidate.parent_id == concept.parent_id)
        .collect::<Vec<_>>();
    siblings.sort_by_key(|candidate| (candidate.position, candidate.id));
    let index = siblings
        .iter()
        .position(|candidate| candidate.id == concept_id)
        .ok_or_else(|| invalid_change("a reordered concept is missing from its sibling list"))?;
    index
        .checked_sub(1)
        .map(|previous| concept_path(snapshot, siblings[previous].id))
        .transpose()
}

fn display_path(path: &[String]) -> String {
    path.iter()
        .map(|segment| format!("{segment:?}"))
        .collect::<Vec<_>>()
        .join(" › ")
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

    use super::{apply_record, resolve_quote, submit_value};
    use crate::change::EvidenceSelector;
    use crate::corpus::{Snapshot, Work, head_snapshot, store_work};

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    #[test]
    fn quote_context_disambiguates_naturally() {
        let work = Work {
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
        assert!(resolve_quote(&work, &ambiguous).is_err());
        let selected = EvidenceSelector {
            within_heading: Some(vec!["Two".to_owned()]),
            ..ambiguous
        };
        assert_eq!(
            &work.text[resolve_quote(&work, &selected).unwrap_or_default().0
                ..resolve_quote(&work, &selected).unwrap_or_default().1],
            "Locks work."
        );
    }

    #[test]
    fn forward_local_references_resolve_independently_of_create_order() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Paper", "Parent claim. Child claim.")?;
        let record = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Create a grounded hierarchy",
                "operations": [
                    {
                        "action": "create_concept",
                        "label": "Child",
                        "under": {"new": "Parent"},
                        "evidence": [{"quote": "Child claim."}]
                    },
                    {
                        "action": "create_concept",
                        "label": "Parent",
                        "evidence": [{"quote": "Parent claim."}]
                    }
                ]
            }),
            "test",
            None,
        )?;
        assert_eq!(apply_record(&mut connection, &record)?, 1);
        let snapshot = head_snapshot(&connection)?;
        let child = snapshot
            .concepts
            .iter()
            .find(|concept| concept.label == "Child")
            .ok_or("missing child")?;
        let parent = snapshot
            .concepts
            .iter()
            .find(|concept| concept.label == "Parent")
            .ok_or("missing parent")?;
        assert_eq!(child.parent_id, Some(parent.id));
        Ok(())
    }

    #[test]
    fn retiring_a_branch_accepts_children_listed_before_the_parent() -> TestResult {
        let mut connection = seeded_hierarchy()?;
        let work = store_work(
            &mut connection,
            "Retirement",
            "Retire the represented branch.",
        )?;
        let record = submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Retire a represented branch",
                "operations": [
                    {"action": "retire_concept", "concept": {"path": ["Parent"]}},
                    {"action": "retire_concept", "concept": {"path": ["Parent", "Child"]}}
                ]
            }),
            "test",
            None,
        )?;
        assert_eq!(apply_record(&mut connection, &record)?, 2);
        assert_eq!(head_snapshot(&connection)?, Snapshot::empty());
        Ok(())
    }

    #[test]
    fn move_then_retire_transition_is_independent_of_operation_order() -> TestResult {
        let mut connection = seeded_hierarchy()?;
        let work = store_work(
            &mut connection,
            "Merge",
            "Replacement claim. The child belongs under the replacement.",
        )?;
        let record = submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Replace a parent while preserving its child",
                "operations": [
                    {"action": "retire_concept", "concept": {"path": ["Parent"]}, "replacement": {"new": "Replacement"}},
                    {"action": "move_concept", "concept": {"path": ["Parent", "Child"]}, "under": {"new": "Replacement"}},
                    {"action": "create_concept", "label": "Replacement", "evidence": [{"quote": "Replacement claim."}]}
                ]
            }),
            "test",
            None,
        )?;
        assert_eq!(apply_record(&mut connection, &record)?, 2);
        let snapshot = head_snapshot(&connection)?;
        assert_eq!(snapshot.concepts.len(), 2);
        let child = snapshot
            .concepts
            .iter()
            .find(|concept| concept.label == "Child")
            .ok_or("missing child")?;
        let replacement = snapshot
            .concepts
            .iter()
            .find(|concept| concept.label == "Replacement")
            .ok_or("missing replacement")?;
        assert_eq!(child.parent_id, Some(replacement.id));
        Ok(())
    }

    #[test]
    fn contradictory_retire_and_move_is_rejected() -> TestResult {
        let mut connection = seeded_hierarchy()?;
        let work = store_work(&mut connection, "Contradiction", "Contradictory request.")?;
        let result = submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Contradictory transition",
                "operations": [
                    {"action": "retire_concept", "concept": {"path": ["Parent", "Child"]}},
                    {"action": "move_concept", "concept": {"path": ["Parent", "Child"]}}
                ]
            }),
            "test",
            None,
        );
        let Err(error) = result else {
            return Err("contradictory transition unexpectedly resolved".into());
        };
        assert_eq!(error.code(), "invalid_reconciliation");
        assert_eq!(head_snapshot(&connection)?.concepts.len(), 2);
        Ok(())
    }

    #[test]
    fn contradictory_ordering_constraints_are_rejected() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Ordering", "First claim. Second claim.")?;
        let result = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Contradictory ordering",
                "operations": [
                    {
                        "action": "create_concept",
                        "label": "First",
                        "before": {"new": "Second"},
                        "evidence": [{"quote": "First claim."}]
                    },
                    {
                        "action": "create_concept",
                        "label": "Second",
                        "before": {"new": "First"},
                        "evidence": [{"quote": "Second claim."}]
                    }
                ]
            }),
            "test",
            None,
        );
        let Err(error) = result else {
            return Err("contradictory ordering unexpectedly resolved".into());
        };
        assert_eq!(error.code(), "invalid_placement");
        assert!(head_snapshot(&connection)?.concepts.is_empty());
        Ok(())
    }

    #[test]
    fn coherent_ordering_constraints_are_resolved_as_one_final_order() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(
            &mut connection,
            "Ordering chain",
            "Alpha claim. Beta claim. Gamma claim.",
        )?;
        let record = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Create a constrained ordering",
                "operations": [
                    {"action": "create_concept", "label": "Alpha", "after": {"new": "Beta"}, "evidence": [{"quote": "Alpha claim."}]},
                    {"action": "create_concept", "label": "Beta", "after": {"new": "Gamma"}, "evidence": [{"quote": "Beta claim."}]},
                    {"action": "create_concept", "label": "Gamma", "evidence": [{"quote": "Gamma claim."}]}
                ]
            }),
            "test",
            None,
        )?;
        apply_record(&mut connection, &record)?;
        let snapshot = head_snapshot(&connection)?;
        let mut labels = snapshot.concepts.iter().collect::<Vec<_>>();
        labels.sort_by_key(|concept| concept.position);
        assert_eq!(
            labels
                .into_iter()
                .map(|concept| concept.label.as_str())
                .collect::<Vec<_>>(),
            ["Gamma", "Beta", "Alpha"]
        );
        Ok(())
    }

    #[test]
    fn superseded_reconciliation_cannot_be_applied_from_a_stale_record() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Superseded", "First claim. Second claim.")?;
        let old = submit_value(
            &mut connection,
            &work,
            0,
            create_root_reconciliation("First", "First claim."),
            "test",
            None,
        )?;
        let current = submit_value(
            &mut connection,
            &work,
            0,
            create_root_reconciliation("Second", "Second claim."),
            "test",
            None,
        )?;
        let Err(error) = apply_record(&mut connection, &old) else {
            return Err("a superseded reconciliation unexpectedly applied".into());
        };
        assert_eq!(error.code(), "nothing_to_apply");
        assert_eq!(apply_record(&mut connection, &current)?, 1);
        Ok(())
    }

    #[test]
    fn late_older_examinations_do_not_supersede_a_newer_pending_change() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(
            &mut connection,
            "Concurrent examination",
            "Seed claim. Current claim. Late claim.",
        )?;
        let seed = submit_value(
            &mut connection,
            &work,
            0,
            create_root_reconciliation("Seed", "Seed claim."),
            "test",
            None,
        )?;
        apply_record(&mut connection, &seed)?;

        let current = submit_value(
            &mut connection,
            &work,
            1,
            create_root_reconciliation("Current", "Current claim."),
            "test",
            None,
        )?;
        assert_eq!(current.status, "pending");

        let late_change = submit_value(
            &mut connection,
            &work,
            0,
            create_root_reconciliation("Late", "Late claim."),
            "test",
            None,
        )?;
        assert_eq!(late_change.status, "superseded");
        let selected = crate::corpus::select_reconciliation(
            &connection,
            Some("Concurrent examination"),
            false,
        )?;
        assert_eq!(selected.id, current.id);
        assert_eq!(
            connection.query_row(
                "SELECT id FROM reconciliations WHERE work_id = ?1 AND status = 'pending'",
                [work.id],
                |row| row.get::<_, i64>(0),
            )?,
            current.id
        );
        assert_eq!(apply_record(&mut connection, &current)?, 2);
        Ok(())
    }

    #[test]
    fn an_equal_projection_is_recorded_without_a_revision() -> TestResult {
        let mut connection = seeded_hierarchy()?;
        let work = store_work(
            &mut connection,
            "Equivalent placement",
            "No additional evidence.",
        )?;
        let record = submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Move the sole child to the same place",
                "operations": [{
                    "action": "move_concept",
                    "concept": {"path": ["Parent", "Child"]},
                    "under": {"path": ["Parent"]}
                }]
            }),
            "test",
            None,
        )?;
        assert_eq!(record.status, "recorded");
        assert_eq!(crate::corpus::revision(&connection)?, 1);
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM commits", [], |row| row
                .get::<_, i64>(0))?,
            1
        );
        connection.execute(
            "UPDATE reconciliations SET status = 'pending' WHERE id = ?1",
            [record.id],
        )?;
        let pending =
            crate::corpus::select_reconciliation(&connection, Some("Equivalent placement"), true)?;
        let Err(error) = apply_record(&mut connection, &pending) else {
            return Err("an equal projection unexpectedly created a revision".into());
        };
        assert_eq!(error.code(), "invalid_resolved_reconciliation");
        assert_eq!(crate::corpus::revision(&connection)?, 1);
        Ok(())
    }

    #[test]
    fn sibling_reorder_receipt_contains_semantic_neighbor_context() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Roots", "First claim. Second claim.")?;
        let seed = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Create roots",
                "operations": [
                    {"action": "create_concept", "label": "First", "evidence": [{"quote": "First claim."}]},
                    {"action": "create_concept", "label": "Second", "evidence": [{"quote": "Second claim."}]}
                ]
            }),
            "test",
            None,
        )?;
        apply_record(&mut connection, &seed)?;
        let reorder = submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Reorder roots",
                "operations": [{
                    "action": "move_concept",
                    "concept": {"path": ["Second"]},
                    "before": {"path": ["First"]}
                }]
            }),
            "test",
            None,
        )?;
        let resolved = super::validate_record(&connection, &reorder)?;
        assert!(matches!(
            &resolved.operations[0],
            super::ResolvedOperation::MoveConcept {
                before,
                after,
                previous_sibling_before: Some(previous),
                previous_sibling_after: None,
            } if before == &["Second"] && after == &["Second"] && previous == &["First"]
        ));
        Ok(())
    }

    #[test]
    fn reverting_reword_after_later_evidence_conflicts_atomically() -> TestResult {
        let mut connection = test_connection()?;
        let work = store_work(
            &mut connection,
            "Evolving claim",
            "Old evidence. New evidence.",
        )?;
        let create = submit_value(
            &mut connection,
            &work,
            0,
            create_root_reconciliation("A", "Old evidence."),
            "test",
            None,
        )?;
        apply_record(&mut connection, &create)?;
        let reword = submit_value(
            &mut connection,
            &work,
            1,
            json!({
                "summary": "Reword A as B",
                "operations": [{
                    "action": "reword_concept",
                    "concept": {"path": ["A"]},
                    "label": "B",
                    "evidence_disposition": "retain"
                }]
            }),
            "test",
            None,
        )?;
        apply_record(&mut connection, &reword)?;
        let evidence = submit_value(
            &mut connection,
            &work,
            2,
            json!({
                "summary": "Support B with later evidence",
                "operations": [{
                    "action": "add_evidence",
                    "concept": {"path": ["B"]},
                    "evidence": [{"quote": "New evidence."}]
                }]
            }),
            "test",
            None,
        )?;
        apply_record(&mut connection, &evidence)?;
        let head_before = head_snapshot(&connection)?;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let Err(error) = crate::corpus::revert(&transaction, 2) else {
            return Err("reword with later evidence unexpectedly reverted".into());
        };
        assert_eq!(error.code(), "revert_conflict");
        transaction.rollback()?;
        assert_eq!(crate::corpus::revision(&connection)?, 3);
        assert_eq!(head_snapshot(&connection)?, head_before);
        assert_eq!(
            connection.query_row("SELECT COUNT(*) FROM commits", [], |row| row
                .get::<_, i64>(0))?,
            3
        );
        Ok(())
    }

    fn seeded_hierarchy() -> TestResult<Connection> {
        let mut connection = test_connection()?;
        let work = store_work(&mut connection, "Seed", "Parent claim. Child claim.")?;
        let record = submit_value(
            &mut connection,
            &work,
            0,
            json!({
                "summary": "Seed hierarchy",
                "operations": [
                    {
                        "action": "create_concept",
                        "label": "Parent",
                        "evidence": [{"quote": "Parent claim."}]
                    },
                    {
                        "action": "create_concept",
                        "label": "Child",
                        "under": {"new": "Parent"},
                        "evidence": [{"quote": "Child claim."}]
                    }
                ]
            }),
            "test",
            None,
        )?;
        apply_record(&mut connection, &record)?;
        Ok(connection)
    }

    fn create_root_reconciliation(label: &str, quote: &str) -> serde_json::Value {
        json!({
            "summary": format!("Create {label}"),
            "operations": [{
                "action": "create_concept",
                "label": label,
                "evidence": [{"quote": quote}]
            }]
        })
    }

    fn test_connection() -> TestResult<Connection> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(include_str!("../schema.sql"))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        Ok(connection)
    }
}
