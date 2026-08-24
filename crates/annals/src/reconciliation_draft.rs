#![allow(clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::change::{
    ChangeOperation, ConceptSelector, EvidenceSelector, parse_operation_value,
    validate_reconciliation_metadata,
};
use crate::corpus::{ReconciliationRecord, Snapshot, Work, heading_for_offset, now, snapshot_at};
use crate::error::AppError;
use crate::resolver;

const MAX_HINT_CHARACTERS: usize = 2_000;
const MAX_HINT_CANDIDATES: usize = 5;
const MAX_CONTEXT_CHARACTERS: usize = 80;
const MAX_EVIDENCE_BYTES: usize = 8_192;
const MAX_STATUS_HINT_CHARACTERS: usize = 16_000;

pub(crate) struct DraftResult {
    pub(crate) output: Value,
    pub(crate) reconciliation: Option<ReconciliationRecord>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmitArgs {
    summary: String,
    operations: Vec<Value>,
    #[serde(default)]
    annotations: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviseArgs {
    expected_version: i64,
    #[serde(default)]
    replace: Vec<Replacement>,
    #[serde(default)]
    remove: Vec<String>,
    #[serde(default)]
    append: Vec<Value>,
    summary: Option<String>,
    annotations: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Replacement {
    operation_id: String,
    operation: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusArgs {
    #[serde(default)]
    operation_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscardArgs {
    expected_version: i64,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Clone)]
struct Draft {
    id: i64,
    version: i64,
    summary: String,
    annotations: Vec<String>,
}

#[derive(Clone)]
struct Slot {
    id: i64,
    operation: Value,
    status: String,
    hint: Option<String>,
}

struct Assessment {
    state: &'static str,
    hint: Option<String>,
    parsed: Option<ChangeOperation>,
}

pub(crate) fn start(
    transaction: &Transaction<'_>,
    run_id: i64,
    work: &Work,
    base_revision: i64,
    sequence: i64,
    arguments: &Value,
    actor: &str,
) -> Result<DraftResult, AppError> {
    let args: SubmitArgs = decode(arguments.clone(), "initial reconciliation")?;
    validate_reconciliation_metadata(&args.summary, &args.annotations)
        .map_err(|error| invalid(error.to_string()))?;
    if args.operations.is_empty() {
        return Err(invalid("A reconciliation needs at least one operation."));
    }
    if open_draft(transaction, run_id)?.is_some() {
        return Err(AppError::conflict(
            "reconciliation_draft_open",
            "A reconciliation draft is already open. Revise it, inspect it, or discard it before starting another.",
        ));
    }

    let version = transaction.query_row(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM reconciliation_drafts \
         WHERE model_run_id = ?1",
        [run_id],
        |row| row.get::<_, i64>(0),
    )?;
    if version < 1 {
        return Err(identity_overflow());
    }
    let timestamp = now()?;
    transaction.execute(
        "INSERT INTO reconciliation_drafts(\
             model_run_id, work_id, base_revision, status, version, summary, annotations, \
             created_sequence, created_at, updated_at\
         ) VALUES(?1, ?2, ?3, 'open', ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            run_id,
            work.id,
            base_revision,
            version,
            args.summary,
            serde_json::to_string(&args.annotations)?,
            sequence,
            timestamp
        ],
    )?;
    let draft_id = transaction.last_insert_rowid();
    for (index, operation) in args.operations.iter().enumerate() {
        let slot = i64::try_from(index)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or_else(identity_overflow)?;
        transaction.execute(
            "INSERT INTO reconciliation_draft_operations(\
                 draft_id, slot, ordinal, operation, status, hint, created_version, \
                 last_changed_version\
             ) VALUES(?1, ?2, ?3, ?4, 'needs_revision', NULL, ?5, ?5)",
            params![
                draft_id,
                slot,
                i64::try_from(index).map_err(|_| identity_overflow())?,
                serde_json::to_string(operation)?,
                version
            ],
        )?;
    }
    evaluate(
        transaction,
        run_id,
        work,
        base_revision,
        sequence,
        draft_id,
        actor,
    )
}

pub(crate) fn revise(
    transaction: &Transaction<'_>,
    run_id: i64,
    work: &Work,
    base_revision: i64,
    sequence: i64,
    arguments: &Value,
    actor: &str,
) -> Result<DraftResult, AppError> {
    let args: ReviseArgs = decode(arguments.clone(), "reconciliation revision")?;
    if args.expected_version < 1 {
        return Err(invalid(
            "expected_version must be a positive draft version.",
        ));
    }
    if args.replace.is_empty()
        && args.remove.is_empty()
        && args.append.is_empty()
        && args.summary.is_none()
        && args.annotations.is_none()
    {
        return Err(invalid(
            "A revision must replace, remove, or append an operation, or explicitly update the summary or annotations.",
        ));
    }
    let draft = require_open_draft(transaction, run_id)?;
    ensure_version(&draft, args.expected_version)?;

    let replacement_slots = args
        .replace
        .iter()
        .map(|replacement| parse_operation_id(&replacement.operation_id))
        .collect::<Result<Vec<_>, _>>()?;
    let removed_slots = args
        .remove
        .iter()
        .map(|operation_id| parse_operation_id(operation_id))
        .collect::<Result<Vec<_>, _>>()?;
    ensure_unique(
        &replacement_slots,
        "An operation can be replaced only once per revision.",
    )?;
    ensure_unique(
        &removed_slots,
        "An operation can be removed only once per revision.",
    )?;
    if replacement_slots
        .iter()
        .any(|slot| removed_slots.contains(slot))
    {
        return Err(invalid(
            "The same operation cannot be replaced and removed in one revision.",
        ));
    }
    for slot in replacement_slots.iter().chain(&removed_slots) {
        require_active_slot(transaction, draft.id, *slot)?;
    }

    let active_count = active_slot_count(transaction, draft.id)?;
    let resulting_count = active_count
        .checked_sub(i64::try_from(removed_slots.len()).map_err(|_| identity_overflow())?)
        .and_then(|count| count.checked_add(i64::try_from(args.append.len()).ok()?))
        .ok_or_else(identity_overflow)?;
    if resulting_count < 1 {
        return Err(invalid(
            "A reconciliation draft must retain at least one operation. Discard the draft to abandon the complete request set.",
        ));
    }

    let version = draft.version.checked_add(1).ok_or_else(identity_overflow)?;
    for (replacement, slot) in args.replace.iter().zip(replacement_slots) {
        transaction.execute(
            "UPDATE reconciliation_draft_operations \
             SET operation = ?1, status = 'needs_revision', hint = NULL, last_changed_version = ?2 \
             WHERE draft_id = ?3 AND slot = ?4 AND status <> 'dropped'",
            params![
                serde_json::to_string(&replacement.operation)?,
                version,
                draft.id,
                slot
            ],
        )?;
    }
    for slot in removed_slots {
        transaction.execute(
            "UPDATE reconciliation_draft_operations \
             SET status = 'dropped', hint = NULL, last_changed_version = ?1 \
             WHERE draft_id = ?2 AND slot = ?3 AND status <> 'dropped'",
            params![version, draft.id, slot],
        )?;
    }
    let mut next_slot = transaction.query_row(
        "SELECT COALESCE(MAX(slot), 0) + 1 FROM reconciliation_draft_operations WHERE draft_id = ?1",
        [draft.id],
        |row| row.get::<_, i64>(0),
    )?;
    let mut next_ordinal = transaction.query_row(
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM reconciliation_draft_operations WHERE draft_id = ?1",
        [draft.id],
        |row| row.get::<_, i64>(0),
    )?;
    for operation in &args.append {
        transaction.execute(
            "INSERT INTO reconciliation_draft_operations(\
                 draft_id, slot, ordinal, operation, status, hint, created_version, \
                 last_changed_version\
             ) VALUES(?1, ?2, ?3, ?4, 'needs_revision', NULL, ?5, ?5)",
            params![
                draft.id,
                next_slot,
                next_ordinal,
                serde_json::to_string(operation)?,
                version
            ],
        )?;
        next_slot = next_slot.checked_add(1).ok_or_else(identity_overflow)?;
        next_ordinal = next_ordinal.checked_add(1).ok_or_else(identity_overflow)?;
    }

    let summary = args.summary.as_deref().unwrap_or(&draft.summary);
    let annotations = args.annotations.as_ref().unwrap_or(&draft.annotations);
    validate_reconciliation_metadata(summary, annotations)
        .map_err(|error| invalid(error.to_string()))?;
    transaction.execute(
        "UPDATE reconciliation_drafts \
         SET version = ?1, summary = ?2, annotations = ?3, updated_at = ?4 \
         WHERE id = ?5 AND status = 'open' AND version = ?6",
        params![
            version,
            summary,
            serde_json::to_string(annotations)?,
            now()?,
            draft.id,
            draft.version
        ],
    )?;
    evaluate(
        transaction,
        run_id,
        work,
        base_revision,
        sequence,
        draft.id,
        actor,
    )
}

pub(crate) fn discard(
    transaction: &Transaction<'_>,
    run_id: i64,
    sequence: i64,
    arguments: &Value,
) -> Result<Value, AppError> {
    let args: DiscardArgs = decode(arguments.clone(), "draft discard")?;
    let draft = require_open_draft(transaction, run_id)?;
    ensure_version(&draft, args.expected_version)?;
    if args
        .reason
        .as_deref()
        .is_some_and(|reason| reason.trim().is_empty())
    {
        return Err(invalid("A discard reason, when supplied, cannot be empty."));
    }
    let version = draft.version.checked_add(1).ok_or_else(identity_overflow)?;
    let timestamp = now()?;
    transaction.execute(
        "UPDATE reconciliation_drafts \
         SET status = 'discarded', version = ?1, terminal_sequence = ?2, \
             updated_at = ?3, completed_at = ?3 \
         WHERE id = ?4 AND status = 'open' AND version = ?5",
        params![version, sequence, timestamp, draft.id, draft.version],
    )?;
    Ok(json!({
        "recorded": false,
        "state": "discarded",
        "draft_version": version,
        "message": "The complete staged request set was discarded. No reconciliation record or corpus change was created. A fresh reconciliation may now be started.",
        "reason": args.reason
    }))
}

pub(crate) fn status(
    connection: &Connection,
    run_id: i64,
    arguments: Value,
) -> Result<Value, AppError> {
    let args: StatusArgs = decode(arguments, "reconciliation status")?;
    if args.operation_ids.len() > 20 {
        return Err(invalid(
            "Request exact content for at most 20 staged operations at a time.",
        ));
    }
    let requested = args
        .operation_ids
        .iter()
        .map(|operation_id| parse_operation_id(operation_id))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let Some(draft) = open_draft(connection, run_id)? else {
        let recorded = connection
            .query_row(
                "SELECT r.id, r.status FROM reconciliations AS r \
                 WHERE r.model_run_id = ?1 ORDER BY r.id DESC LIMIT 1",
                [run_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        return Ok(recorded.map_or_else(
            || {
                json!({
                    "recorded": false,
                    "state": "empty",
                    "message": "There is no open reconciliation draft."
                })
            },
            |(id, reconciliation_status)| {
                json!({
                    "recorded": true,
                    "state": "recorded",
                    "reconciliation_id": id,
                    "reconciliation_status": reconciliation_status,
                    "message": "This examination has already recorded its reconciliation."
                })
            },
        ));
    };
    if !requested.is_empty() {
        for slot in &requested {
            require_slot(connection, draft.id, *slot)?;
        }
    }
    status_value(connection, &draft, &requested)
}

pub(crate) fn abandon_open_for_run(
    connection: &Connection,
    run_id: i64,
    timestamp: &str,
) -> Result<(), AppError> {
    connection.execute(
        "UPDATE reconciliation_drafts \
         SET status = 'abandoned', updated_at = ?1, completed_at = ?1 \
         WHERE model_run_id = ?2 AND status = 'open'",
        params![timestamp, run_id],
    )?;
    Ok(())
}

fn evaluate(
    transaction: &Transaction<'_>,
    run_id: i64,
    work: &Work,
    base_revision: i64,
    sequence: i64,
    draft_id: i64,
    actor: &str,
) -> Result<DraftResult, AppError> {
    let draft = draft_by_id(transaction, draft_id)?;
    let slots = load_slots(transaction, draft_id, false)?;
    let base = snapshot_at(transaction, base_revision)?;
    let assessments = assess_slots(work, &base, &slots);
    for (slot, assessment) in slots.iter().zip(&assessments) {
        transaction.execute(
            "UPDATE reconciliation_draft_operations SET status = ?1, hint = ?2 \
             WHERE draft_id = ?3 AND slot = ?4",
            params![assessment.state, assessment.hint, draft_id, slot.id],
        )?;
    }
    if assessments
        .iter()
        .any(|assessment| assessment.state != "staged")
    {
        let refreshed = draft_by_id(transaction, draft_id)?;
        return Ok(DraftResult {
            output: status_value(transaction, &refreshed, &BTreeSet::new())?,
            reconciliation: None,
        });
    }

    let request = assembled_request(&draft, &slots);
    match resolver::submit_value_in_transaction(
        transaction,
        work,
        base_revision,
        request,
        actor,
        Some(run_id),
        Some(draft_id),
    ) {
        Ok(record) => {
            let timestamp = now()?;
            transaction.execute(
                "UPDATE reconciliation_drafts \
                 SET status = 'finalized', terminal_sequence = ?1, updated_at = ?2, \
                     completed_at = ?2 \
                 WHERE id = ?3 AND status = 'open'",
                params![sequence, timestamp, draft_id],
            )?;
            Ok(DraftResult {
                output: json!({
                    "recorded": true,
                    "state": "recorded",
                    "draft_version": draft.version,
                    "reconciliation_id": record.id,
                    "work": record.work_label,
                    "base_revision": record.base_revision,
                    "accepted_operations": slots.len(),
                    "summary": record.summary,
                    "reconciliation_status": record.status,
                    "message": "All staged operations were accepted and one complete reconciliation was recorded."
                }),
                reconciliation: Some(record),
            })
        }
        Err(error) if repairable(&error) => {
            let implicated = implicated_slots(error.code(), &slots);
            let names = implicated
                .iter()
                .map(|slot| operation_id(*slot))
                .collect::<Vec<_>>()
                .join(", ");
            let hint = bounded(
                &format!(
                    "These staged operations cannot be recorded together: {error}. Review {names}. Every other staged operation remains preserved."
                ),
                MAX_HINT_CHARACTERS,
            );
            for slot in &implicated {
                transaction.execute(
                    "UPDATE reconciliation_draft_operations \
                     SET status = 'implicated', hint = ?1 WHERE draft_id = ?2 AND slot = ?3",
                    params![hint, draft_id, slot],
                )?;
            }
            let refreshed = draft_by_id(transaction, draft_id)?;
            Ok(DraftResult {
                output: status_value(transaction, &refreshed, &BTreeSet::new())?,
                reconciliation: None,
            })
        }
        Err(error) => Err(error),
    }
}

fn assess_slots(work: &Work, base: &Snapshot, slots: &[Slot]) -> Vec<Assessment> {
    let mut assessments = slots
        .iter()
        .map(|slot| match parse_operation_value(slot.operation.clone()) {
            Ok(operation) => Assessment {
                state: "staged",
                hint: None,
                parsed: Some(operation),
            },
            Err(error) => Assessment {
                state: "needs_revision",
                hint: Some(bounded(
                    &format!(
                        "{} could not be understood as one complete reconciliation operation: {}. Replace only {}; the other operations remain preserved.",
                        operation_id(slot.id),
                        error,
                        operation_id(slot.id)
                    ),
                    MAX_HINT_CHARACTERS,
                )),
                parsed: None,
            },
        })
        .collect::<Vec<_>>();

    let mut declarations = BTreeMap::<String, Vec<usize>>::new();
    for (index, slot) in slots.iter().enumerate() {
        if let Some(handle) = raw_creation_handle(&slot.operation) {
            declarations
                .entry(handle.to_owned())
                .or_default()
                .push(index);
        }
    }
    for (handle, owners) in &declarations {
        for owner in owners.iter().skip(1) {
            add_issue(
                &mut assessments[*owner],
                "needs_revision",
                format!(
                    "{} repeats the local creation handle {handle:?}, first used by {}. Give this creation a distinct ref; the first declaration remains staged.",
                    operation_id(slots[*owner].id),
                    operation_id(slots[owners[0]].id)
                ),
            );
        }
    }

    let base_ids = base
        .concepts
        .iter()
        .map(|concept| concept.id)
        .collect::<BTreeSet<_>>();
    let base_edges = base
        .edges
        .iter()
        .map(|edge| (edge.parent_id, edge.child_id))
        .collect::<BTreeSet<_>>();
    for index in 0..slots.len() {
        let Some(operation) = assessments[index].parsed.clone() else {
            continue;
        };
        for selector in selectors(&operation) {
            match selector {
                ConceptSelector::Existing { id } if !base_ids.contains(&id.storage_id()) => {
                    add_issue(
                        &mut assessments[index],
                        "needs_revision",
                        format!(
                            "{} refers to {}, but that concept is not present in the frozen base revision. Inspect the corpus and replace only this operation.",
                            operation_id(slots[index].id),
                            id
                        ),
                    );
                }
                ConceptSelector::New { handle } => match declarations.get(handle) {
                    None => add_issue(
                        &mut assessments[index],
                        "needs_revision",
                        format!(
                            "{} refers to the local handle {handle:?}, but no create_concept operation declares it. Correct the handle or append its creation.",
                            operation_id(slots[index].id)
                        ),
                    ),
                    Some(owners)
                        if owners.len() > 1 || assessments[owners[0]].state == "needs_revision" =>
                    {
                        if owners[0] != index {
                            add_issue(
                                &mut assessments[index],
                                "blocked",
                                format!(
                                    "{} is preserved and will be checked automatically after {}'s creation handle is corrected.",
                                    operation_id(slots[index].id),
                                    operation_id(slots[owners[0]].id)
                                ),
                            );
                        }
                    }
                    Some(_) => {}
                },
                ConceptSelector::Existing { .. } => {}
            }
        }
        for (evidence_index, evidence) in evidence(&operation).iter().enumerate() {
            if let Some(problem) = quote_problem(work, evidence) {
                add_issue(
                    &mut assessments[index],
                    "needs_revision",
                    format!(
                        "{} evidence quotation {} needs attention. {problem}",
                        operation_id(slots[index].id),
                        evidence_index + 1
                    ),
                );
            }
        }
        assess_local_semantics(
            &operation,
            work,
            base,
            &base_edges,
            &mut assessments[index],
            slots[index].id,
        );
    }
    assess_cross_operation_conflicts(slots, &mut assessments);
    assessments
}

fn assess_local_semantics(
    operation: &ChangeOperation,
    work: &Work,
    base: &Snapshot,
    base_edges: &BTreeSet<(i64, i64)>,
    assessment: &mut Assessment,
    operation_slot: i64,
) {
    match operation {
        ChangeOperation::CreateConcept { parents, .. } => {
            let unique = parents.iter().collect::<BTreeSet<_>>();
            if unique.len() != parents.len() {
                add_issue(
                    assessment,
                    "needs_revision",
                    format!(
                        "{} names the same broader parent more than once. Keep each parent only once.",
                        operation_id(operation_slot)
                    ),
                );
            }
        }
        ChangeOperation::AddParent { concept, parent } => {
            if concept == parent {
                add_issue(
                    assessment,
                    "needs_revision",
                    format!(
                        "{} makes a concept its own broader parent. Replace or remove this operation.",
                        operation_id(operation_slot)
                    ),
                );
            }
        }
        ChangeOperation::RemoveParent { concept, parent } => {
            if let (
                ConceptSelector::Existing { id: concept },
                ConceptSelector::Existing { id: parent },
            ) = (concept, parent)
                && !base_edges.contains(&(parent.storage_id(), concept.storage_id()))
            {
                add_issue(
                    assessment,
                    "needs_revision",
                    format!(
                        "{} asks to remove the direct broader/narrower link {} → {}, but that link is not present in the frozen base revision.",
                        operation_id(operation_slot),
                        parent,
                        concept
                    ),
                );
            } else if matches!(concept, ConceptSelector::New { .. })
                || matches!(parent, ConceptSelector::New { .. })
            {
                add_issue(
                    assessment,
                    "needs_revision",
                    format!(
                        "{} cannot remove a broader/narrower link involving a concept created in this request, because that link is not present in the frozen base revision.",
                        operation_id(operation_slot)
                    ),
                );
            }
        }
        ChangeOperation::RemoveEvidence {
            concept: ConceptSelector::Existing { id },
            evidence,
        } => {
            for selector in evidence {
                let matches = filtered_matches(work, selector);
                if let [(start, end)] = matches.as_slice()
                    && !base.evidence.iter().any(|item| {
                        item.concept_id == id.storage_id()
                            && item.work_id == work.id
                            && item.start_byte == *start
                            && item.end_byte == *end
                    })
                {
                    add_issue(
                        assessment,
                        "needs_revision",
                        format!(
                            "{} asks to remove a quotation that is not attached to {} in the frozen base revision.",
                            operation_id(operation_slot),
                            id
                        ),
                    );
                }
            }
        }
        ChangeOperation::RemoveEvidence {
            concept: ConceptSelector::New { .. },
            ..
        } => add_issue(
            assessment,
            "needs_revision",
            format!(
                "{} cannot remove evidence from a concept created in this request, because that evidence is not attached in the frozen base revision.",
                operation_id(operation_slot)
            ),
        ),
        ChangeOperation::RewordConcept { concept, .. } => {
            if matches!(concept, ConceptSelector::New { .. }) {
                add_issue(
                    assessment,
                    "needs_revision",
                    format!(
                        "{} targets a concept created in this same request. Supply its final wording and relationships in create_concept instead.",
                        operation_id(operation_slot)
                    ),
                );
            }
        }
        ChangeOperation::RetireConcept {
            concept,
            replacement,
        } => {
            if matches!(concept, ConceptSelector::New { .. }) {
                add_issue(
                    assessment,
                    "needs_revision",
                    format!(
                        "{} targets a concept created in this same request. Supply its final wording and relationships in create_concept instead.",
                        operation_id(operation_slot)
                    ),
                );
            }
            if replacement.as_ref() == Some(concept) {
                add_issue(
                    assessment,
                    "needs_revision",
                    format!(
                        "{} names the retired concept as its own replacement. Choose a different surviving concept or omit the replacement.",
                        operation_id(operation_slot)
                    ),
                );
            }
        }
        ChangeOperation::AddEvidence { .. } => {}
    }
}

#[allow(clippy::type_complexity)]
fn assess_cross_operation_conflicts(slots: &[Slot], assessments: &mut [Assessment]) {
    let mut retirements = BTreeMap::<ConceptSelector, Vec<usize>>::new();
    let mut rewordings = BTreeMap::<ConceptSelector, Vec<usize>>::new();
    let mut edge_adds = BTreeMap::<(ConceptSelector, ConceptSelector), Vec<usize>>::new();
    let mut edge_removes = BTreeMap::<(ConceptSelector, ConceptSelector), Vec<usize>>::new();
    let mut evidence_adds = BTreeMap::<(ConceptSelector, EvidenceSelector), Vec<usize>>::new();
    let mut evidence_removes = BTreeMap::<(ConceptSelector, EvidenceSelector), Vec<usize>>::new();

    for (index, assessment) in assessments.iter().enumerate() {
        let Some(operation) = &assessment.parsed else {
            continue;
        };
        match operation {
            ChangeOperation::CreateConcept {
                handle,
                parents,
                evidence,
                ..
            } => {
                let concept = ConceptSelector::New {
                    handle: handle.clone(),
                };
                for parent in parents {
                    edge_adds
                        .entry((parent.clone(), concept.clone()))
                        .or_default()
                        .push(index);
                }
                for selector in evidence {
                    evidence_adds
                        .entry((concept.clone(), selector.clone()))
                        .or_default()
                        .push(index);
                }
            }
            ChangeOperation::AddParent { concept, parent } => {
                edge_adds
                    .entry((parent.clone(), concept.clone()))
                    .or_default()
                    .push(index);
            }
            ChangeOperation::RemoveParent { concept, parent } => {
                edge_removes
                    .entry((parent.clone(), concept.clone()))
                    .or_default()
                    .push(index);
            }
            ChangeOperation::AddEvidence { concept, evidence } => {
                for selector in evidence {
                    evidence_adds
                        .entry((concept.clone(), selector.clone()))
                        .or_default()
                        .push(index);
                }
            }
            ChangeOperation::RemoveEvidence { concept, evidence } => {
                for selector in evidence {
                    evidence_removes
                        .entry((concept.clone(), selector.clone()))
                        .or_default()
                        .push(index);
                }
            }
            ChangeOperation::RewordConcept { concept, .. } => {
                rewordings.entry(concept.clone()).or_default().push(index);
            }
            ChangeOperation::RetireConcept { concept, .. } => {
                retirements.entry(concept.clone()).or_default().push(index);
            }
        }
    }

    for indices in retirements.values().filter(|indices| indices.len() > 1) {
        mark_conflict(
            slots,
            assessments,
            indices,
            "These operations retire the same concept more than once. Keep or revise only one retirement.",
        );
    }
    for indices in rewordings.values().filter(|indices| indices.len() > 1) {
        mark_conflict(
            slots,
            assessments,
            indices,
            "These operations reword the same concept more than once. Combine them into one final wording.",
        );
    }
    for (concept, retired_indices) in &retirements {
        if let Some(reworded_indices) = rewordings.get(concept) {
            let indices = retired_indices
                .iter()
                .chain(reworded_indices)
                .copied()
                .collect::<Vec<_>>();
            mark_conflict(
                slots,
                assessments,
                &indices,
                "These operations both retire and reword the same concept. Choose the intended final action.",
            );
        }
        let conflicting_uses = assessments
            .iter()
            .enumerate()
            .filter_map(|(index, assessment)| {
                (!retired_indices.contains(&index)
                    && assessment
                        .parsed
                        .as_ref()
                        .is_some_and(|operation| selectors(operation).contains(&concept)))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        for index in conflicting_uses {
            let indices = retired_indices
                .iter()
                .copied()
                .chain(std::iter::once(index))
                .collect::<Vec<_>>();
            mark_conflict(
                slots,
                assessments,
                &indices,
                "One operation retires a concept that another operation also changes, relates, or uses as a replacement. Choose a consistent final use of that concept.",
            );
        }
    }
    mark_map_duplicates(
        slots,
        assessments,
        &edge_removes,
        "These operations remove the same broader/narrower link more than once.",
    );
    mark_map_duplicates(
        slots,
        assessments,
        &evidence_removes,
        "These operations remove the same evidence more than once.",
    );
    mark_map_intersections(
        slots,
        assessments,
        &edge_adds,
        &edge_removes,
        "These operations add and remove the same broader/narrower link. Choose one final result.",
    );
    mark_map_intersections(
        slots,
        assessments,
        &evidence_adds,
        &evidence_removes,
        "These operations add and remove the same evidence. Choose one final result.",
    );
}

fn mark_map_duplicates<K: Ord>(
    slots: &[Slot],
    assessments: &mut [Assessment],
    map: &BTreeMap<K, Vec<usize>>,
    message: &str,
) {
    for indices in map.values().filter(|indices| indices.len() > 1) {
        mark_conflict(slots, assessments, indices, message);
    }
}

fn mark_map_intersections<K: Ord>(
    slots: &[Slot],
    assessments: &mut [Assessment],
    left: &BTreeMap<K, Vec<usize>>,
    right: &BTreeMap<K, Vec<usize>>,
    message: &str,
) {
    for (key, left_indices) in left {
        if let Some(right_indices) = right.get(key) {
            let indices = left_indices
                .iter()
                .chain(right_indices)
                .copied()
                .collect::<Vec<_>>();
            mark_conflict(slots, assessments, &indices, message);
        }
    }
}

fn mark_conflict(slots: &[Slot], assessments: &mut [Assessment], indices: &[usize], message: &str) {
    if indices
        .iter()
        .any(|index| assessments[*index].state != "staged")
    {
        return;
    }
    let names = indices
        .iter()
        .map(|index| operation_id(slots[*index].id))
        .collect::<Vec<_>>()
        .join(", ");
    for index in indices {
        add_issue(
            &mut assessments[*index],
            "implicated",
            format!("{message} Review {names}; every other staged operation remains preserved."),
        );
    }
}

fn quote_problem(work: &Work, selector: &EvidenceSelector) -> Option<String> {
    if selector.quote.len() > MAX_EVIDENCE_BYTES {
        return Some(format!(
            "The exact quotation is {} UTF-8 bytes; evidence quotations may contain at most {MAX_EVIDENCE_BYTES}. Select a shorter exact passage.",
            selector.quote.len()
        ));
    }
    let raw = work
        .text
        .match_indices(&selector.quote)
        .map(|(start, _)| (start, start + selector.quote.len()))
        .collect::<Vec<_>>();
    let mut candidates = raw.clone();
    let mut eliminated_by = None;
    if let Some(heading) = &selector.within_heading {
        retain_heading(work, &mut candidates, heading);
        if candidates.is_empty() && !raw.is_empty() {
            eliminated_by = Some(format!(
                "The quote exists, but none of its occurrences is under the submitted heading {}.",
                display_heading(Some(heading))
            ));
        }
    }
    if let Some(prefix) = &selector.preceded_by {
        let before = candidates.len();
        candidates.retain(|(start, _)| work.text[..*start].ends_with(prefix));
        if candidates.is_empty() && before > 0 {
            eliminated_by = Some(format!(
                "The quote exists in the selected region, but {:?} is not immediately before it.",
                bounded(prefix, 120)
            ));
        }
    }
    if let Some(suffix) = &selector.followed_by {
        let before = candidates.len();
        candidates.retain(|(_, end)| work.text[*end..].starts_with(suffix));
        if candidates.is_empty() && before > 0 {
            eliminated_by = Some(format!(
                "The quote exists in the selected region, but {:?} is not immediately after it.",
                bounded(suffix, 120)
            ));
        }
    }
    match candidates.len() {
        1 => None,
        0 if raw.is_empty() => Some(quote_absent_hint(work, selector)),
        0 => {
            let examples = candidate_hints(work, selector, &raw);
            Some(format!(
                "{} Here are the actual source locations:\n{}",
                eliminated_by.unwrap_or_else(|| {
                    "The exact quote occurs in the work, but the supplied context filters exclude every occurrence.".to_owned()
                }),
                examples
            ))
        }
        count => Some(format!(
            "The exact quotation still matches {count} locations after applying its context. Add the heading or exact immediately adjacent words to select one:\n{}",
            candidate_hints(work, selector, &candidates)
        )),
    }
}

fn quote_absent_hint(work: &Work, selector: &EvidenceSelector) -> String {
    let submitted = bounded(&selector.quote, 240);
    match nearby_source(work, &selector.quote) {
        Some((start, excerpt)) => format!(
            "I could not find the submitted quotation exactly.\nSubmitted: {submitted:?}\nNearby source under {}: {:?}\nUse exact source text; Annals did not substitute this nearby passage.",
            display_heading(heading_for_offset(&work.text, start).as_ref()),
            excerpt
        ),
        None => format!(
            "I could not find the submitted quotation exactly, and no sufficiently long exact fragment pointed to a nearby passage. Submitted: {submitted:?}. Read or search the source again and copy its exact wording."
        ),
    }
}

fn candidate_hints(
    work: &Work,
    selector: &EvidenceSelector,
    candidates: &[(usize, usize)],
) -> String {
    let mut lines = Vec::new();
    for (number, (start, end)) in candidates.iter().take(MAX_HINT_CANDIDATES).enumerate() {
        let before = chars_before(&work.text, *start, MAX_CONTEXT_CHARACTERS);
        let after = chars_after(&work.text, *end, MAX_CONTEXT_CHARACTERS);
        let matched = bounded(&work.text[*start..*end], 160);
        let suggestion = suggested_selector(work, selector, *start, *end)
            .and_then(|value| serde_json::to_string(&value).ok())
            .map_or_else(
                || "Use a longer exact quotation from this location.".to_owned(),
                |value| format!("A selector verified for this location is {value}."),
            );
        lines.push(format!(
            "{}. Under {}: …{}[{}]{}… {suggestion}",
            number + 1,
            display_heading(heading_for_offset(&work.text, *start).as_ref()),
            before,
            matched,
            after
        ));
    }
    if candidates.len() > MAX_HINT_CANDIDATES {
        lines.push(format!(
            "{} additional matching locations are not shown.",
            candidates.len() - MAX_HINT_CANDIDATES
        ));
    }
    bounded(&lines.join("\n"), MAX_HINT_CHARACTERS)
}

fn suggested_selector(
    work: &Work,
    original: &EvidenceSelector,
    start: usize,
    end: usize,
) -> Option<Value> {
    let heading = heading_for_offset(&work.text, start);
    if let Some(path) = &heading {
        let candidate = EvidenceSelector {
            quote: original.quote.clone(),
            within_heading: Some(path.clone()),
            preceded_by: None,
            followed_by: None,
        };
        if filtered_matches(work, &candidate).len() == 1 {
            return serde_json::to_value(candidate).ok();
        }
    }
    for width in [16_usize, 32, 64, 128] {
        let prefix = chars_before(&work.text, start, width);
        let suffix = chars_after(&work.text, end, width);
        for (preceded_by, followed_by) in [
            (nonempty(prefix.clone()), None),
            (None, nonempty(suffix.clone())),
            (nonempty(prefix.clone()), nonempty(suffix.clone())),
        ] {
            let candidate = EvidenceSelector {
                quote: original.quote.clone(),
                within_heading: heading.clone(),
                preceded_by,
                followed_by,
            };
            if filtered_matches(work, &candidate).len() == 1 {
                return serde_json::to_value(candidate).ok();
            }
        }
    }
    None
}

fn filtered_matches(work: &Work, selector: &EvidenceSelector) -> Vec<(usize, usize)> {
    let mut candidates = work
        .text
        .match_indices(&selector.quote)
        .map(|(start, _)| (start, start + selector.quote.len()))
        .collect::<Vec<_>>();
    if let Some(heading) = &selector.within_heading {
        retain_heading(work, &mut candidates, heading);
    }
    if let Some(prefix) = &selector.preceded_by {
        candidates.retain(|(start, _)| work.text[..*start].ends_with(prefix));
    }
    if let Some(suffix) = &selector.followed_by {
        candidates.retain(|(_, end)| work.text[*end..].starts_with(suffix));
    }
    candidates
}

fn retain_heading(work: &Work, candidates: &mut Vec<(usize, usize)>, heading: &[String]) {
    let normalized = heading
        .iter()
        .map(|component| crate::index::normalize(component))
        .collect::<Vec<_>>();
    candidates.retain(|(start, _)| {
        heading_for_offset(&work.text, *start).is_some_and(|path| {
            path.iter()
                .map(|component| crate::index::normalize(component))
                .eq(normalized.iter().cloned())
        })
    });
}

fn nearby_source(work: &Work, quote: &str) -> Option<(usize, String)> {
    let boundaries = quote
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(quote.len()))
        .collect::<Vec<_>>();
    let characters = boundaries.len().saturating_sub(1);
    for width in [80_usize, 48, 32, 20, 12] {
        if width > characters {
            continue;
        }
        let mut starts = vec![0, (characters - width) / 2, characters - width];
        starts.sort_unstable();
        starts.dedup();
        for character_start in starts {
            let fragment = &quote[boundaries[character_start]..boundaries[character_start + width]];
            if let Some(match_start) = work.text.find(fragment) {
                let excerpt_start = floor_char_boundary(
                    &work.text,
                    match_start.saturating_sub(MAX_CONTEXT_CHARACTERS),
                );
                let excerpt_end = ceil_char_boundary(
                    &work.text,
                    (match_start + fragment.len() + MAX_CONTEXT_CHARACTERS).min(work.text.len()),
                );
                return Some((
                    match_start,
                    bounded(work.text[excerpt_start..excerpt_end].trim(), 320),
                ));
            }
        }
    }
    None
}

fn status_value(
    connection: &Connection,
    draft: &Draft,
    exact: &BTreeSet<i64>,
) -> Result<Value, AppError> {
    let slots = load_slots(connection, draft.id, true)?;
    let mut counts = BTreeMap::<&str, usize>::new();
    let operations = slots
        .iter()
        .map(|slot| {
            let public_status = public_status(&slot.status);
            *counts.entry(public_status).or_default() += 1;
            let mut item = json!({
                "operation_id": operation_id(slot.id),
                "status": public_status,
                "summary": operation_summary(&slot.operation)
            });
            if exact.contains(&slot.id) {
                item["operation"] = slot.operation.clone();
                if let Some(hint) = &slot.hint {
                    item["hint"] = json!(hint);
                }
            }
            item
        })
        .collect::<Vec<_>>();
    let mut shown_hint_characters = 0_usize;
    let mut hints_not_shown = 0_usize;
    let attention = slots
        .iter()
        .filter(|slot| matches!(slot.status.as_str(), "needs_revision" | "implicated"))
        .map(|slot| {
            let mut item = json!({
                "operation_id": operation_id(slot.id),
                "status": public_status(&slot.status)
            });
            if let Some(hint) = &slot.hint {
                let hint_characters = hint.chars().count();
                if exact.contains(&slot.id)
                    || shown_hint_characters.saturating_add(hint_characters)
                        <= MAX_STATUS_HINT_CHARACTERS
                {
                    item["hint"] = json!(hint);
                    shown_hint_characters = shown_hint_characters.saturating_add(hint_characters);
                } else {
                    hints_not_shown += 1;
                }
            }
            item
        })
        .collect::<Vec<_>>();
    let staged_ids = operations
        .iter()
        .filter(|operation| operation["status"] == "staged")
        .filter_map(|operation| operation.get("operation_id").cloned())
        .collect::<Vec<_>>();
    let waiting_ids = operations
        .iter()
        .filter(|operation| operation["status"] == "waiting")
        .filter_map(|operation| operation.get("operation_id").cloned())
        .collect::<Vec<_>>();
    Ok(json!({
        "recorded": false,
        "state": "needs_changes",
        "draft_version": draft.version,
        "summary": draft.summary,
        "counts": {
            "active": slots.iter().filter(|slot| slot.status != "dropped").count(),
            "staged": counts.get("staged").copied().unwrap_or(0),
            "needs_attention": counts.get("needs attention").copied().unwrap_or(0),
            "waiting": counts.get("waiting").copied().unwrap_or(0),
            "conflict": counts.get("semantic conflict").copied().unwrap_or(0),
            "removed": counts.get("removed").copied().unwrap_or(0)
        },
        "staged_operation_ids": staged_ids,
        "waiting_operation_ids": waiting_ids,
        "operations": operations,
        "attention": attention,
        "hints_not_shown": hints_not_shown,
        "next": if hints_not_shown == 0 {
            "Revise only operations listed under attention. Staged operations and waiting dependencies are preserved automatically. Use reconciliation_status with operation_ids to retrieve exact stored operations."
        } else {
            "Revise only operations whose hints are shown. Other operations remain preserved. Ask reconciliation_status for named operation_ids to retrieve omitted hints and exact stored operations."
        }
    }))
}

fn assembled_request(draft: &Draft, slots: &[Slot]) -> Value {
    json!({
        "summary": draft.summary,
        "operations": slots
            .iter()
            .filter(|slot| slot.status != "dropped")
            .map(|slot| slot.operation.clone())
            .collect::<Vec<_>>(),
        "annotations": draft.annotations
    })
}

fn load_slots(
    connection: &Connection,
    draft_id: i64,
    include_dropped: bool,
) -> Result<Vec<Slot>, AppError> {
    let sql = if include_dropped {
        "SELECT slot, operation, status, hint \
         FROM reconciliation_draft_operations WHERE draft_id = ?1 ORDER BY ordinal"
    } else {
        "SELECT slot, operation, status, hint \
         FROM reconciliation_draft_operations \
         WHERE draft_id = ?1 AND status <> 'dropped' ORDER BY ordinal"
    };
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([draft_id], |row| {
        let operation = row.get::<_, String>(1)?;
        Ok(Slot {
            id: row.get(0)?,
            operation: serde_json::from_str(&operation).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
            status: row.get(2)?,
            hint: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn open_draft(connection: &Connection, run_id: i64) -> Result<Option<Draft>, AppError> {
    connection
        .query_row(
            "SELECT id, version, summary, annotations FROM reconciliation_drafts \
             WHERE model_run_id = ?1 AND status = 'open'",
            [run_id],
            draft_from_row,
        )
        .optional()
        .map_err(AppError::from)
}

fn require_open_draft(connection: &Connection, run_id: i64) -> Result<Draft, AppError> {
    open_draft(connection, run_id)?.ok_or_else(|| {
        AppError::not_found(
            "reconciliation_draft_not_found",
            "There is no open reconciliation draft. Start one with submit_reconciliation.",
        )
    })
}

fn draft_by_id(connection: &Connection, draft_id: i64) -> Result<Draft, AppError> {
    connection
        .query_row(
            "SELECT id, version, summary, annotations FROM reconciliation_drafts WHERE id = ?1",
            [draft_id],
            draft_from_row,
        )
        .map_err(AppError::from)
}

fn draft_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Draft> {
    let annotations = row.get::<_, String>(3)?;
    Ok(Draft {
        id: row.get(0)?,
        version: row.get(1)?,
        summary: row.get(2)?,
        annotations: serde_json::from_str(&annotations).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
    })
}

fn require_active_slot(connection: &Connection, draft_id: i64, slot: i64) -> Result<(), AppError> {
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM reconciliation_draft_operations \
         WHERE draft_id = ?1 AND slot = ?2 AND status <> 'dropped')",
        params![draft_id, slot],
        |row| row.get::<_, bool>(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(AppError::not_found(
            "staged_operation_not_found",
            format!(
                "{} is not an active operation in this reconciliation draft.",
                operation_id(slot)
            ),
        ))
    }
}

fn require_slot(connection: &Connection, draft_id: i64, slot: i64) -> Result<(), AppError> {
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM reconciliation_draft_operations \
         WHERE draft_id = ?1 AND slot = ?2)",
        params![draft_id, slot],
        |row| row.get::<_, bool>(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(AppError::not_found(
            "staged_operation_not_found",
            format!("{} is not part of this draft.", operation_id(slot)),
        ))
    }
}

fn active_slot_count(connection: &Connection, draft_id: i64) -> Result<i64, AppError> {
    Ok(connection.query_row(
        "SELECT COUNT(*) FROM reconciliation_draft_operations \
         WHERE draft_id = ?1 AND status <> 'dropped'",
        [draft_id],
        |row| row.get(0),
    )?)
}

fn ensure_version(draft: &Draft, expected: i64) -> Result<(), AppError> {
    if draft.version == expected {
        Ok(())
    } else {
        Err(AppError::conflict(
            "stale_reconciliation_draft",
            format!(
                "The draft is now version {}, not version {expected}. Read reconciliation_status and retry against the current version.",
                draft.version
            ),
        ))
    }
}

fn ensure_unique(values: &[i64], message: &str) -> Result<(), AppError> {
    let unique = values.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() == values.len() {
        Ok(())
    } else {
        Err(invalid(message))
    }
}

fn selectors(operation: &ChangeOperation) -> Vec<&ConceptSelector> {
    match operation {
        ChangeOperation::CreateConcept { parents, .. } => parents.iter().collect(),
        ChangeOperation::AddParent { concept, parent }
        | ChangeOperation::RemoveParent { concept, parent } => vec![concept, parent],
        ChangeOperation::AddEvidence { concept, .. }
        | ChangeOperation::RemoveEvidence { concept, .. }
        | ChangeOperation::RewordConcept { concept, .. } => vec![concept],
        ChangeOperation::RetireConcept {
            concept,
            replacement,
        } => std::iter::once(concept).chain(replacement.iter()).collect(),
    }
}

fn evidence(operation: &ChangeOperation) -> &[EvidenceSelector] {
    match operation {
        ChangeOperation::CreateConcept { evidence, .. }
        | ChangeOperation::AddEvidence { evidence, .. }
        | ChangeOperation::RemoveEvidence { evidence, .. } => evidence,
        ChangeOperation::AddParent { .. }
        | ChangeOperation::RemoveParent { .. }
        | ChangeOperation::RewordConcept { .. }
        | ChangeOperation::RetireConcept { .. } => &[],
    }
}

fn raw_creation_handle(operation: &Value) -> Option<&str> {
    (operation.get("action").and_then(Value::as_str) == Some("create_concept"))
        .then(|| operation.get("ref").and_then(Value::as_str))
        .flatten()
}

fn add_issue(assessment: &mut Assessment, state: &'static str, message: impl Into<String>) {
    if assessment.state != "needs_revision" || state == "needs_revision" {
        assessment.state = state;
    }
    let message = message.into();
    assessment.hint = Some(bounded(
        &assessment
            .hint
            .as_ref()
            .map_or(message.clone(), |existing| format!("{existing}\n{message}")),
        MAX_HINT_CHARACTERS,
    ));
}

fn implicated_slots(code: &str, slots: &[Slot]) -> BTreeSet<i64> {
    let actions: &[&str] = match code {
        "would_create_cycle" => &[
            "create_concept",
            "add_parent",
            "remove_parent",
            "retire_concept",
        ],
        "ungrounded_leaf" => &[
            "create_concept",
            "remove_parent",
            "remove_evidence",
            "reword_concept",
            "retire_concept",
        ],
        _ => &[],
    };
    let selected = slots
        .iter()
        .filter(|slot| {
            actions.is_empty()
                || slot
                    .operation
                    .get("action")
                    .and_then(Value::as_str)
                    .is_some_and(|action| actions.contains(&action))
        })
        .map(|slot| slot.id)
        .collect::<BTreeSet<_>>();
    if selected.is_empty() {
        slots.iter().map(|slot| slot.id).collect()
    } else {
        selected
    }
}

fn repairable(error: &AppError) -> bool {
    matches!(
        error,
        AppError::Invalid { .. } | AppError::NotFound { .. } | AppError::Conflict { .. }
    )
}

fn public_status(status: &str) -> &'static str {
    match status {
        "staged" => "staged",
        "blocked" => "waiting",
        "implicated" => "semantic conflict",
        "dropped" => "removed",
        _ => "needs attention",
    }
}

fn operation_summary(operation: &Value) -> String {
    let action = operation
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("unrecognized operation");
    let summary = match action {
        "create_concept" => format!(
            "Create concept {:?}",
            operation
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("without a readable label")
        ),
        "add_parent" => "Add one broader/narrower link".to_owned(),
        "remove_parent" => "Remove one broader/narrower link".to_owned(),
        "add_evidence" => "Attach exact evidence".to_owned(),
        "remove_evidence" => "Remove exact evidence".to_owned(),
        "reword_concept" => format!(
            "Reword a concept to {:?}",
            operation
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("an unreadable label")
        ),
        "retire_concept" => "Retire one concept".to_owned(),
        other => format!("Unrecognized operation action {other:?}"),
    };
    bounded(&summary, 200)
}

fn display_heading(path: Option<&Vec<String>>) -> String {
    path.filter(|path| !path.is_empty()).map_or_else(
        || "text outside a heading".to_owned(),
        |path| format!("“{}”", path.join(" › ")),
    )
}

fn operation_id(slot: i64) -> String {
    format!("op-{slot}")
}

fn parse_operation_id(value: &str) -> Result<i64, AppError> {
    let Some(digits) = value.strip_prefix("op-") else {
        return Err(invalid(format!(
            "Operation ID {value:?} is not one returned by Annals; expected a value such as op-3."
        )));
    };
    let slot = digits.parse::<i64>().map_err(|_| {
        invalid(format!(
            "Operation ID {value:?} is not one returned by Annals; expected a value such as op-3."
        ))
    })?;
    if slot > 0 {
        Ok(slot)
    } else {
        Err(invalid(format!(
            "Operation ID {value:?} is not one returned by Annals; expected a value such as op-3."
        )))
    }
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value, context: &str) -> Result<T, AppError> {
    serde_json::from_value(value).map_err(|error| {
        invalid(format!(
            "The {context} call could not be read: {}",
            bounded(&error.to_string(), 400)
        ))
    })
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::invalid("invalid_reconciliation_draft", message)
}

fn identity_overflow() -> AppError {
    AppError::database(
        "identity_overflow",
        "reconciliation draft identity space is exhausted",
    )
}

fn bounded(text: &str, limit: usize) -> String {
    let mut characters = text.chars();
    let mut value = characters.by_ref().take(limit).collect::<String>();
    if characters.next().is_some() {
        value.push('…');
    }
    value
}

fn nonempty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn chars_before(text: &str, end: usize, limit: usize) -> String {
    let portion = &text[..end];
    let start = portion
        .char_indices()
        .rev()
        .nth(limit.saturating_sub(1))
        .map_or(0, |(offset, _)| offset);
    portion[start..].to_owned()
}

fn chars_after(text: &str, start: usize, limit: usize) -> String {
    let portion = &text[start..];
    let end = portion
        .char_indices()
        .nth(limit)
        .map_or(portion.len(), |(offset, _)| offset);
    portion[..end].to_owned()
}

fn floor_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn ceil_char_boundary(text: &str, mut offset: usize) -> usize {
    offset = offset.min(text.len());
    while offset < text.len() && !text.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::store_work;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn quotation_hint_distinguishes_a_bad_qualifier_from_missing_text() -> TestResult {
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch(include_str!("../schema.sql"))?;
        let work = store_work(
            &mut connection,
            "Hints",
            "# One\n\nBefore exact words after.\n\n# Two\n\nOther exact words later.",
        )?;
        let qualified = EvidenceSelector {
            quote: "exact words".to_owned(),
            within_heading: Some(vec!["One".to_owned()]),
            preceded_by: Some("wrong ".to_owned()),
            followed_by: None,
        };
        let hint = quote_problem(&work, &qualified).ok_or("expected qualifier hint")?;
        assert!(hint.contains("not immediately before"));
        assert!(hint.contains("actual source locations"));

        let absent = EvidenceSelector {
            quote: "Before exact word after.".to_owned(),
            within_heading: None,
            preceded_by: None,
            followed_by: None,
        };
        let hint = quote_problem(&work, &absent).ok_or("expected absent hint")?;
        assert!(hint.contains("could not find"));
        assert!(hint.contains("Nearby source"));
        Ok(())
    }

    #[test]
    fn ambiguous_quotation_suggestions_are_verified_before_they_are_shown() -> TestResult {
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch(include_str!("../schema.sql"))?;
        let work = store_work(
            &mut connection,
            "Suggestions",
            "# One\n\nBefore repeated words after one.\n\n# Two\n\nOther repeated words after two.",
        )?;
        let selector = EvidenceSelector {
            quote: "repeated words".to_owned(),
            within_heading: None,
            preceded_by: None,
            followed_by: None,
        };
        let matches = filtered_matches(&work, &selector);
        assert_eq!(matches.len(), 2);
        for (start, end) in matches {
            let suggestion = suggested_selector(&work, &selector, start, end)
                .ok_or("expected a unique suggested selector")?;
            let suggestion: EvidenceSelector = serde_json::from_value(suggestion)?;
            assert_eq!(filtered_matches(&work, &suggestion).len(), 1);
        }
        let hint = quote_problem(&work, &selector).ok_or("expected ambiguity hint")?;
        assert!(hint.contains("matches 2 locations"));
        assert!(hint.contains("selector verified for this location"));
        Ok(())
    }

    #[test]
    fn removals_from_new_concepts_are_local_problems() -> TestResult {
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch(include_str!("../schema.sql"))?;
        let work = store_work(&mut connection, "New removals", "Exact source.")?;
        let base = snapshot_at(&connection, 0)?;
        let slots = vec![
            Slot {
                id: 1,
                operation: json!({
                    "action": "create_concept",
                    "ref": "new_one",
                    "label": "New one",
                    "parents": [],
                    "evidence": [{"quote": "Exact source."}]
                }),
                status: "needs_revision".to_owned(),
                hint: None,
            },
            Slot {
                id: 2,
                operation: json!({
                    "action": "remove_evidence",
                    "concept": {"new": "new_one"},
                    "evidence": [{"quote": "Exact source."}]
                }),
                status: "needs_revision".to_owned(),
                hint: None,
            },
            Slot {
                id: 3,
                operation: json!({
                    "action": "remove_parent",
                    "concept": {"new": "new_one"},
                    "parent": {"new": "new_one"}
                }),
                status: "needs_revision".to_owned(),
                hint: None,
            },
        ];
        let assessments = assess_slots(&work, &base, &slots);
        assert_eq!(assessments[0].state, "staged");
        assert_eq!(assessments[1].state, "needs_revision");
        assert_eq!(assessments[2].state, "needs_revision");
        assert!(
            assessments[1]
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("frozen base revision"))
        );
        Ok(())
    }
}
