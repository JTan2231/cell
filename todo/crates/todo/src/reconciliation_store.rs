use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension as _, Row, Transaction, TransactionBehavior, params};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::model::{
    ConcernId, DesignId, RoutingProposalId, SituationAssessmentId, TodoId, TodoStatus,
    WorkingNoteId,
};
use crate::tool_server::contracts::{
    AssessmentDisposition, BoundaryDisposition, ConcernRoutingProposal, DesignClauseKind,
    DesignRevision, DesignSubmission, DirectionBoundaryAttribution, DirectionBoundaryKind,
    FindingKind, JurisdictionAction, JurisdictionAssignment, JurisdictionRole, NewDesignClause,
    NewJurisdictionChange, RoutingDisposition, SituationAssessment, UnresolvedKind,
};

const PROMPT_ROUTING: &str = "todo/concern-routing/1";
const PROMPT_ASSESSMENT: &str = "todo/situation-assessment/1";
const PROMPT_DESIGN: &str = "todo/design-reconciliation/1";
const REQUIRED_DESIGN_CLAUSE_KINDS: [&str; 9] = [
    "ownership",
    "boundary",
    "state",
    "interface",
    "lifecycle",
    "failure",
    "compatibility",
    "acceptance",
    "non_goal",
];

type AssignmentTuple = (String, String, String);
type JurisdictionAssignments = BTreeMap<String, Vec<AssignmentTuple>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ConcernStatus {
    Pending,
    Attached,
    Dismissed,
}

impl ConcernStatus {
    fn parse(value: &str) -> rusqlite::Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "attached" => Ok(Self::Attached),
            "dismissed" => Ok(Self::Dismissed),
            _ => Err(invalid_stored_text("invalid concern status")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Concern {
    pub(crate) id: ConcernId,
    pub(crate) body: String,
    pub(crate) source_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_item_id: Option<String>,
    pub(crate) status: ConcernStatus,
    pub(crate) created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DirectionBoundary {
    pub(crate) id: i64,
    pub(crate) local_ref: String,
    pub(crate) kind: String,
    pub(crate) statement: String,
    pub(crate) attribution: String,
    pub(crate) source_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DirectionRevision {
    pub(crate) id: i64,
    pub(crate) todo_id: TodoId,
    pub(crate) revision: i64,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) provenance_kind: String,
    pub(crate) created_at: String,
    pub(crate) boundaries: Vec<DirectionBoundary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RoutingTargetView {
    pub(crate) todo_id: TodoId,
    pub(crate) direction_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RoutingProposalView {
    pub(crate) id: RoutingProposalId,
    pub(crate) concern_id: ConcernId,
    pub(crate) action: String,
    pub(crate) targets: Vec<RoutingTargetView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) survivor_todo_id: Option<TodoId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) proposed_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) proposed_direction: Option<String>,
    pub(crate) proposed_boundaries: Vec<DirectionBoundary>,
    pub(crate) rationale: String,
    pub(crate) evidence_refs: Vec<String>,
    pub(crate) limitations: Vec<String>,
    pub(crate) decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) decision_source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) decision_thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) decision_turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) decision_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) decided_at: Option<String>,
    pub(crate) created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RoutingCandidate {
    pub(crate) id: TodoId,
    pub(crate) title: String,
    pub(crate) direction: String,
    pub(crate) direction_revision: i64,
    pub(crate) status: TodoStatus,
    pub(crate) boundaries: Vec<DirectionBoundary>,
}

#[derive(Debug, Clone)]
pub(crate) struct RoutingSnapshot {
    pub(crate) concern: Concern,
    pub(crate) base_digest: String,
    pub(crate) candidates: Vec<RoutingCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DecisionSource {
    pub(crate) source_path: String,
    pub(crate) thread_id: Option<String>,
    pub(crate) turn_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RoutingDecision {
    pub(crate) proposal: RoutingProposalView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) todo_id: Option<TodoId>,
    pub(crate) changed: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentJob {
    pub(crate) id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AssessmentBase {
    pub(crate) source_ref: String,
    pub(crate) kind: String,
    pub(crate) locator: String,
    pub(crate) revision: String,
    pub(crate) observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DesignCorrection {
    pub(crate) agent_job_id: i64,
    pub(crate) based_on_design_id: DesignId,
    pub(crate) feedback: String,
    pub(crate) basis_ref: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AssessmentSnapshot {
    pub(crate) todo_id: TodoId,
    pub(crate) direction: DirectionRevision,
    pub(crate) concern_set_digest: String,
    pub(crate) notes_through_id: Option<i64>,
    pub(crate) based_on_design_id: Option<DesignId>,
    pub(crate) base_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SituationAssessmentView {
    pub(crate) id: SituationAssessmentId,
    pub(crate) todo_id: TodoId,
    pub(crate) direction_revision: i64,
    #[serde(skip)]
    direction_revision_id: i64,
    pub(crate) concern_set_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) notes_through_id: Option<WorkingNoteId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) based_on_design_id: Option<DesignId>,
    pub(crate) disposition: String,
    pub(crate) summary: String,
    pub(crate) subject_label: String,
    pub(crate) observed_at: String,
    pub(crate) current: bool,
    pub(crate) stale_reasons: Vec<String>,
    pub(crate) bases: Vec<AssessmentBase>,
    pub(crate) identity_refs: Vec<String>,
    pub(crate) findings: Vec<serde_json::Value>,
    pub(crate) jurisdictions: Vec<serde_json::Value>,
    pub(crate) direction_mappings: Vec<serde_json::Value>,
    pub(crate) unresolved: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DesignView {
    pub(crate) id: DesignId,
    pub(crate) todo_id: TodoId,
    pub(crate) revision: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) assessment_id: Option<SituationAssessmentId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) based_on_design_id: Option<DesignId>,
    pub(crate) draft_version: i64,
    pub(crate) state: String,
    pub(crate) summary: String,
    pub(crate) current: bool,
    pub(crate) stale_reasons: Vec<String>,
    pub(crate) jurisdiction_changes: Vec<serde_json::Value>,
    pub(crate) clauses: Vec<serde_json::Value>,
    pub(crate) unresolved_choices: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) correction_basis_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) correction_feedback: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) decision_source_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) decision_thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) decision_turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) decision_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) decided_at: Option<String>,
    pub(crate) created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AssessmentReturnView {
    pub(crate) assessment_id: SituationAssessmentId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) design_id: Option<DesignId>,
    pub(crate) reason: String,
    pub(crate) missing_or_stale_refs: Vec<String>,
    pub(crate) producer_tool_call_id: String,
    pub(crate) created_at: String,
}

pub(crate) fn capture_concern(
    connection: &mut Connection,
    body: &str,
    source_path: &Path,
) -> AppResult<Concern> {
    validate_nonblank("concern", body)?;
    let source = absolute_utf8_path(source_path, "source")?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "INSERT INTO concerns(body, source_path) VALUES(?1, ?2)",
        params![body, source],
    )?;
    let id = concern_id(transaction.last_insert_rowid())?;
    let concern = get_concern_tx(&transaction, id)?;
    transaction.commit()?;
    Ok(concern)
}

pub(crate) fn get_concern(connection: &Connection, id: ConcernId) -> AppResult<Concern> {
    get_concern_tx(connection, id)
}

pub(crate) fn list_concerns(
    connection: &Connection,
    include_resolved: bool,
    limit: u32,
) -> AppResult<Vec<Concern>> {
    let mut statement = connection.prepare(
        "SELECT id, body, source_path, source_thread_id, source_turn_id,
                source_item_id, status, created_at, resolved_at
         FROM concerns
         WHERE ?1 OR status = 'pending'
         ORDER BY created_at DESC, id DESC
         LIMIT ?2",
    )?;
    let rows = statement.query_map(
        params![include_resolved, i64::from(limit)],
        concern_from_row,
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(crate) fn routing_snapshot(
    connection: &Connection,
    concern_id: ConcernId,
) -> AppResult<RoutingSnapshot> {
    let concern = get_concern(connection, concern_id)?;
    if concern.status != ConcernStatus::Pending {
        return Err(AppError::conflict(
            "concern_already_resolved",
            format!("concern is no longer pending: {concern_id}"),
        ));
    }
    let candidates = routing_candidates(connection)?;
    let base_digest = routing_digest(&candidates);
    Ok(RoutingSnapshot {
        concern,
        base_digest,
        candidates,
    })
}

pub(crate) fn record_agent_job(
    connection: &mut Connection,
    stage: &str,
    concern_id: Option<ConcernId>,
    todo_id: Option<TodoId>,
    base_digest: &str,
    requester_id: &str,
    nucleus_job_id: &str,
) -> AppResult<AgentJob> {
    validate_nonblank("base digest", base_digest)?;
    validate_nonblank("requester ID", requester_id)?;
    validate_nonblank("Nucleus job ID", nucleus_job_id)?;
    let identity = match stage {
        "concern_routing" => PROMPT_ROUTING,
        "situation_assessment" => PROMPT_ASSESSMENT,
        "design_reconciliation" => PROMPT_DESIGN,
        _ => {
            return Err(AppError::invalid(
                "invalid_agent_stage",
                format!("unsupported Todo agent stage: {stage}"),
            ));
        }
    };
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "INSERT INTO todo_agent_jobs(
             stage, concern_id, todo_id, base_digest, nucleus_requester_id,
             nucleus_job_id, prompt_identity, toolset_identity
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![
            stage,
            concern_id.map(ConcernId::storage_id),
            todo_id.map(TodoId::storage_id),
            base_digest,
            requester_id,
            nucleus_job_id,
            identity,
        ],
    )?;
    let job = AgentJob {
        id: transaction.last_insert_rowid(),
    };
    transaction.commit()?;
    Ok(job)
}

pub(crate) fn record_design_correction(
    connection: &mut Connection,
    agent_job_id: i64,
    based_on_design_id: DesignId,
    feedback: &str,
) -> AppResult<DesignCorrection> {
    validate_nonblank("design correction feedback", feedback)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let job_todo = transaction
        .query_row(
            "SELECT todo_id FROM todo_agent_jobs
             WHERE id = ?1 AND stage = 'design_reconciliation'",
            [agent_job_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten()
        .ok_or_else(|| {
            AppError::conflict(
                "design_correction_job_mismatch",
                "design correction requires a design-reconciliation agent job",
            )
        })?;
    if let Some(existing) = get_design_correction_tx(&transaction, agent_job_id)? {
        if existing.based_on_design_id == based_on_design_id && existing.feedback == feedback {
            transaction.commit()?;
            return Ok(existing);
        }
        return Err(AppError::conflict(
            "design_correction_already_recorded",
            "this design-reconciliation job already has different correction provenance",
        ));
    }
    let predecessor = get_design_tx(&transaction, based_on_design_id)?;
    if predecessor.todo_id.storage_id() != job_todo {
        return Err(AppError::conflict(
            "design_correction_job_mismatch",
            "the correction job and based-on design belong to different todos",
        ));
    }
    if !matches!(
        predecessor.state.as_str(),
        "ready" | "rejected" | "abandoned"
    ) {
        return Err(AppError::conflict(
            "design_not_correctable",
            format!(
                "design cannot be used as a correction basis from state {}",
                predecessor.state
            ),
        ));
    }
    let basis_ref = format!("correction:{agent_job_id}");
    transaction.execute(
        "INSERT INTO todo_design_corrections(
             agent_job_id, based_on_design_id, feedback, basis_ref
         ) VALUES(?1,?2,?3,?4)",
        params![
            agent_job_id,
            based_on_design_id.storage_id(),
            feedback,
            basis_ref,
        ],
    )?;
    let correction = get_design_correction_tx(&transaction, agent_job_id)?
        .ok_or_else(|| stored_data_error("recorded design correction could not be reloaded"))?;
    transaction.commit()?;
    Ok(correction)
}

pub(crate) fn create_routing_proposal(
    connection: &mut Connection,
    snapshot: &RoutingSnapshot,
    agent_job_id: i64,
    tool_call_id: &str,
    proposal: &ConcernRoutingProposal,
) -> AppResult<RoutingProposalView> {
    validate_nonblank("tool call ID", tool_call_id)?;
    let current = routing_snapshot(connection, snapshot.concern.id)?;
    if current.base_digest != snapshot.base_digest {
        return Err(AppError::conflict(
            "routing_basis_changed",
            "todo candidates changed while concern routing was running",
        ));
    }
    validate_routing_targets(snapshot, proposal)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_agent_job(
        &transaction,
        agent_job_id,
        "concern_routing",
        Some(snapshot.concern.id),
        None,
        &snapshot.base_digest,
    )?;
    let action = routing_disposition(proposal.disposition);
    let proposed = proposal.proposed_direction.as_ref();
    let digest = format!("job:{agent_job_id}:call:{tool_call_id}");
    transaction.execute(
        "INSERT INTO concern_routing_proposals(
             concern_id, agent_job_id, action, proposed_title,
             proposed_direction, rationale, proposal_digest,
             producer_tool_call_id
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            snapshot.concern.id.storage_id(),
            agent_job_id,
            action,
            proposed.map(|direction| direction.title.as_str()),
            proposed.map(|direction| direction.body.as_str()),
            proposal.rationale,
            digest,
            tool_call_id,
        ],
    )?;
    let id = routing_id(transaction.last_insert_rowid())?;
    for (ordinal, target) in proposal.targets.iter().enumerate() {
        let todo_id = parse_todo_id(&target.todo_id)?;
        transaction.execute(
            "INSERT INTO concern_routing_targets(
                 routing_id, ordinal, todo_id, direction_revision
             ) VALUES(?1, ?2, ?3, ?4)",
            params![
                id.storage_id(),
                i64::try_from(ordinal).map_err(|_| invalid_number("routing target ordinal"))?,
                todo_id.storage_id(),
                i64::try_from(target.direction_revision)
                    .map_err(|_| invalid_number("direction revision"))?,
            ],
        )?;
    }
    if let Some(unify) = &proposal.unify {
        let survivor = parse_todo_id(&unify.survivor_todo_id)?;
        transaction.execute(
            "INSERT INTO concern_routing_unifications(routing_id, survivor_todo_id)
             VALUES(?1, ?2)",
            params![id.storage_id(), survivor.storage_id()],
        )?;
    }
    if let Some(direction) = proposed {
        insert_routing_boundaries(&transaction, id, &direction.boundaries)?;
    }
    insert_strings(
        &transaction,
        "concern_routing_evidence",
        "evidence_ref",
        id.storage_id(),
        &proposal.evidence_refs,
    )?;
    insert_strings(
        &transaction,
        "concern_routing_limitations",
        "limitation",
        id.storage_id(),
        &proposal.limitations,
    )?;
    validate_routing_shape(&transaction, id, action)?;
    let view = get_routing_tx(&transaction, id)?;
    transaction.commit()?;
    Ok(view)
}

pub(crate) fn get_routing(
    connection: &Connection,
    id: RoutingProposalId,
) -> AppResult<RoutingProposalView> {
    get_routing_tx(connection, id)
}

pub(crate) fn list_routing_for_concern(
    connection: &Connection,
    concern_id: ConcernId,
) -> AppResult<Vec<RoutingProposalView>> {
    let mut statement = connection.prepare(
        "SELECT id FROM concern_routing_proposals
         WHERE concern_id = ?1 ORDER BY id",
    )?;
    let ids = statement
        .query_map([concern_id.storage_id()], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|id| get_routing(connection, routing_id(id)?))
        .collect()
}

pub(crate) fn authorize_routing(
    connection: &mut Connection,
    id: RoutingProposalId,
    source: &DecisionSource,
) -> AppResult<RoutingDecision> {
    validate_decision_source(source)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let before = get_routing_tx(&transaction, id)?;
    if before.decision == "authorized" {
        let todo_id = routed_todo_id(&transaction, &before)?;
        transaction.commit()?;
        return Ok(RoutingDecision {
            proposal: before,
            todo_id,
            changed: false,
        });
    }
    if before.decision != "pending" {
        return Err(AppError::conflict(
            "routing_already_decided",
            format!("routing proposal is already {}: {id}", before.decision),
        ));
    }
    let concern = get_concern_tx(&transaction, before.concern_id)?;
    if concern.status != ConcernStatus::Pending {
        invalidate_routing_tx(&transaction, id, "concern is no longer pending")?;
        transaction.commit()?;
        return Err(AppError::conflict(
            "routing_basis_changed",
            "the concern was resolved after this proposal was created",
        ));
    }
    let stored_digest: String = transaction.query_row(
        "SELECT j.base_digest
         FROM concern_routing_proposals AS r
         JOIN todo_agent_jobs AS j ON j.id = r.agent_job_id
         WHERE r.id = ?1",
        [id.storage_id()],
        |row| row.get(0),
    )?;
    if routing_digest(&routing_candidates(&transaction)?) != stored_digest {
        invalidate_routing_tx(&transaction, id, "candidate todo heads changed")?;
        transaction.commit()?;
        return Err(AppError::conflict(
            "routing_basis_changed",
            "todo candidates changed after this proposal was created; assess the concern again",
        ));
    }
    for target in &before.targets {
        require_exact_direction_revision(&transaction, target.todo_id, target.direction_revision)?;
        require_canonical_open_todo(&transaction, target.todo_id)?;
    }

    let todo_id = match before.action.as_str() {
        "attach" => {
            let target = require_single_target(&before)?;
            attach_concern_tx(&transaction, target.todo_id, before.concern_id, id)?;
            Some(target.todo_id)
        }
        "create" => {
            let todo_id = create_todo_from_routing_tx(&transaction, &before, id)?;
            Some(todo_id)
        }
        "revise" => {
            let target = require_single_target(&before)?;
            append_direction_from_routing_tx(&transaction, target.todo_id, &before, id)?;
            attach_concern_tx(&transaction, target.todo_id, before.concern_id, id)?;
            Some(target.todo_id)
        }
        "unify" => Some(authorize_unification_tx(&transaction, &before, id)?),
        "dismiss" => {
            transaction.execute(
                "UPDATE concerns
                 SET status = 'dismissed',
                     resolved_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                [before.concern_id.storage_id()],
            )?;
            None
        }
        "defer" => None,
        _ => return Err(stored_data_error("invalid routing action")),
    };
    transaction.execute(
        "UPDATE concern_routing_proposals
         SET decision = 'authorized', decision_source_path = ?2,
             decision_thread_id = ?3, decision_turn_id = ?4,
             decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
        params![
            id.storage_id(),
            source.source_path,
            source.thread_id,
            source.turn_id,
        ],
    )?;
    let proposal = get_routing_tx(&transaction, id)?;
    transaction.commit()?;
    Ok(RoutingDecision {
        proposal,
        todo_id,
        changed: true,
    })
}

pub(crate) fn reject_routing(
    connection: &mut Connection,
    id: RoutingProposalId,
    source: &DecisionSource,
    reason: &str,
) -> AppResult<RoutingDecision> {
    validate_decision_source(source)?;
    validate_nonblank("rejection reason", reason)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let before = get_routing_tx(&transaction, id)?;
    if before.decision == "rejected"
        && before.decision_reason.as_deref() == Some(reason)
        && before.decision_source_path.as_deref() == Some(source.source_path.as_str())
    {
        transaction.commit()?;
        return Ok(RoutingDecision {
            proposal: before,
            todo_id: None,
            changed: false,
        });
    }
    if before.decision != "pending" {
        return Err(AppError::conflict(
            "routing_already_decided",
            format!("routing proposal is already {}: {id}", before.decision),
        ));
    }
    transaction.execute(
        "UPDATE concern_routing_proposals
         SET decision = 'rejected', decision_source_path = ?2,
             decision_thread_id = ?3, decision_turn_id = ?4,
             decision_reason = ?5,
             decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
        params![
            id.storage_id(),
            source.source_path,
            source.thread_id,
            source.turn_id,
            reason,
        ],
    )?;
    let proposal = get_routing_tx(&transaction, id)?;
    transaction.commit()?;
    Ok(RoutingDecision {
        proposal,
        todo_id: None,
        changed: true,
    })
}

pub(crate) fn assessment_snapshot(
    connection: &Connection,
    todo_id: TodoId,
) -> AppResult<AssessmentSnapshot> {
    require_canonical_open_todo(connection, todo_id)?;
    let direction = current_direction(connection, todo_id)?;
    let concern_set_digest = concern_set_digest(connection, todo_id)?;
    let notes_through_id = effective_note_cursor(connection, todo_id)?;
    let based_on_design_id = latest_authorized_design_id(connection, todo_id)?;
    let base_digest = format!(
        "todo:{}|direction:{}|concerns:{}|notes:{}|design:{}",
        todo_id.storage_id(),
        direction.id,
        concern_set_digest,
        notes_through_id.map_or_else(|| "-".to_owned(), |id| id.to_string()),
        based_on_design_id.map_or_else(|| "-".to_owned(), |id| id.storage_id().to_string()),
    );
    Ok(AssessmentSnapshot {
        todo_id,
        direction,
        concern_set_digest,
        notes_through_id,
        based_on_design_id,
        base_digest,
    })
}

#[allow(clippy::too_many_lines)] // Keep the assessment's atomic persistence transaction together.
pub(crate) fn create_situation_assessment(
    connection: &mut Connection,
    snapshot: &AssessmentSnapshot,
    agent_job_id: i64,
    tool_call_id: &str,
    assessment: &SituationAssessment,
    bases: &[AssessmentBase],
) -> AppResult<SituationAssessmentView> {
    validate_nonblank("tool call ID", tool_call_id)?;
    let current = assessment_snapshot(connection, snapshot.todo_id)?;
    if current.base_digest != snapshot.base_digest {
        return Err(AppError::conflict(
            "assessment_basis_changed",
            "the todo changed while its situation was being assessed",
        ));
    }
    validate_assessment_refs(snapshot, assessment)?;
    validate_assessment_source_refs(assessment, bases)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_agent_job(
        &transaction,
        agent_job_id,
        "situation_assessment",
        None,
        Some(snapshot.todo_id),
        &snapshot.base_digest,
    )?;
    let observed_at = now(&transaction)?;
    transaction.execute(
        "INSERT INTO todo_situation_assessments(
             todo_id, agent_job_id, direction_revision_id, concern_set_digest,
             notes_through_id, based_on_design_id, disposition, summary,
             subject_label, observed_at, producer_tool_call_id
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            snapshot.todo_id.storage_id(),
            agent_job_id,
            snapshot.direction.id,
            snapshot.concern_set_digest,
            snapshot.notes_through_id,
            snapshot.based_on_design_id.map(DesignId::storage_id),
            assessment_disposition(assessment.disposition),
            assessment.summary,
            assessment.subject.label,
            observed_at,
            tool_call_id,
        ],
    )?;
    let id = assessment_id(transaction.last_insert_rowid())?;
    insert_strings(
        &transaction,
        "todo_assessment_identity_refs",
        "identity_ref",
        id.storage_id(),
        &assessment.subject.identity_refs,
    )?;
    for base in bases {
        validate_nonblank("assessment base source ref", &base.source_ref)?;
        validate_nonblank("assessment base kind", &base.kind)?;
        validate_nonblank("assessment base locator", &base.locator)?;
        validate_nonblank("assessment base revision", &base.revision)?;
        transaction.execute(
            "INSERT INTO todo_assessment_bases(
                 assessment_id, source_ref, kind, locator, revision, observed_at
             ) VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                id.storage_id(),
                base.source_ref,
                base.kind,
                base.locator,
                base.revision,
                base.observed_at
            ],
        )?;
    }
    let mut finding_ids = BTreeMap::new();
    for finding in &assessment.findings {
        transaction.execute(
            "INSERT INTO todo_assessment_findings(
                 assessment_id, local_ref, kind, claim
             ) VALUES(?1,?2,?3,?4)",
            params![
                id.storage_id(),
                finding.r#ref,
                finding_kind(finding.kind),
                finding.claim
            ],
        )?;
        let finding_id = transaction.last_insert_rowid();
        finding_ids.insert(finding.r#ref.clone(), finding_id);
        insert_strings(
            &transaction,
            "todo_assessment_finding_evidence",
            "evidence_ref",
            finding_id,
            &finding.evidence_refs,
        )?;
    }
    for jurisdiction in &assessment.jurisdictions {
        transaction.execute(
            "INSERT INTO todo_assessment_jurisdictions(
                 assessment_id, jurisdiction_key, concern
             ) VALUES(?1,?2,?3)",
            params![id.storage_id(), jurisdiction.key, jurisdiction.concern],
        )?;
        let jurisdiction_id = transaction.last_insert_rowid();
        insert_assignments(
            &transaction,
            "todo_assessment_jurisdiction_assignments",
            "jurisdiction_id",
            jurisdiction_id,
            None,
            &jurisdiction.assignments,
        )?;
        insert_strings(
            &transaction,
            "todo_assessment_jurisdiction_evidence",
            "evidence_ref",
            jurisdiction_id,
            &jurisdiction.evidence_refs,
        )?;
    }
    let boundary_ids = snapshot
        .direction
        .boundaries
        .iter()
        .map(|boundary| (boundary.local_ref.as_str(), boundary.id))
        .collect::<BTreeMap<_, _>>();
    for mapping in &assessment.direction_mappings {
        let boundary_id = *boundary_ids
            .get(mapping.boundary_ref.as_str())
            .ok_or_else(|| {
                AppError::invalid(
                    "unknown_boundary_reference",
                    format!("unknown direction boundary: {}", mapping.boundary_ref),
                )
            })?;
        transaction.execute(
            "INSERT INTO todo_assessment_direction_mappings(
                 assessment_id, boundary_id, disposition, explanation
             ) VALUES(?1,?2,?3,?4)",
            params![
                id.storage_id(),
                boundary_id,
                boundary_disposition(mapping.disposition),
                mapping.explanation
            ],
        )?;
        let mapping_id = transaction.last_insert_rowid();
        for finding_ref in &mapping.finding_refs {
            let finding_id = *finding_ids.get(finding_ref).ok_or_else(|| {
                AppError::invalid(
                    "unknown_finding_reference",
                    format!("unknown assessment finding: {finding_ref}"),
                )
            })?;
            transaction.execute(
                "INSERT INTO todo_assessment_mapping_findings(mapping_id, finding_id)
                 VALUES(?1,?2)",
                params![mapping_id, finding_id],
            )?;
        }
    }
    for unresolved in &assessment.unresolved {
        transaction.execute(
            "INSERT INTO todo_assessment_unresolved(
                 assessment_id, local_ref, kind, description, materiality
             ) VALUES(?1,?2,?3,?4,?5)",
            params![
                id.storage_id(),
                unresolved.r#ref,
                unresolved_kind(unresolved.kind),
                unresolved.description,
                unresolved.materiality
            ],
        )?;
        let unresolved_id = transaction.last_insert_rowid();
        insert_strings(
            &transaction,
            "todo_assessment_unresolved_evidence",
            "evidence_ref",
            unresolved_id,
            &unresolved.evidence_refs,
        )?;
    }
    let view = get_assessment_tx(&transaction, id)?;
    transaction.commit()?;
    Ok(view)
}

pub(crate) fn get_assessment(
    connection: &Connection,
    id: SituationAssessmentId,
) -> AppResult<SituationAssessmentView> {
    get_assessment_tx(connection, id)
}

pub(crate) fn latest_current_ready_assessment(
    connection: &Connection,
    todo_id: TodoId,
) -> AppResult<SituationAssessmentView> {
    let mut statement = connection.prepare(
        "SELECT id FROM todo_situation_assessments
         WHERE todo_id = ?1 AND disposition = 'ready'
           AND NOT EXISTS (
               SELECT 1 FROM todo_designs AS d
               WHERE d.assessment_id = todo_situation_assessments.id
                 AND d.state IN ('open', 'ready', 'authorized')
           )
           AND NOT EXISTS (
               SELECT 1 FROM todo_design_assessment_returns AS returned
               WHERE returned.assessment_id = todo_situation_assessments.id
           )
         ORDER BY id DESC",
    )?;
    let ids = statement
        .query_map([todo_id.storage_id()], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for raw in ids {
        let view = get_assessment(connection, assessment_id(raw)?)?;
        if view.current {
            return Ok(view);
        }
    }
    Err(AppError::conflict(
        "current_ready_assessment_missing",
        format!("{todo_id} has no current ready situation assessment"),
    ))
}

pub(crate) fn create_design(
    connection: &mut Connection,
    assessment: &SituationAssessmentView,
    based_on_design_id: Option<DesignId>,
    agent_job_id: i64,
    tool_call_id: &str,
    submission: &DesignSubmission,
) -> AppResult<DesignView> {
    validate_nonblank("tool call ID", tool_call_id)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let refreshed = get_assessment_tx(&transaction, assessment.id)?;
    if !refreshed.current || refreshed.disposition != "ready" {
        return Err(AppError::conflict(
            "design_basis_changed",
            "the situation assessment is no longer current and ready",
        ));
    }
    validate_design_predecessor(&transaction, &refreshed, based_on_design_id)?;
    let snapshot = assessment_snapshot(&transaction, refreshed.todo_id)?;
    require_agent_job(
        &transaction,
        agent_job_id,
        "design_reconciliation",
        None,
        Some(refreshed.todo_id),
        &snapshot.base_digest,
    )?;
    let admitted_bases =
        design_basis_catalog(&transaction, refreshed.id, based_on_design_id, agent_job_id)?;
    validate_design_refs(&transaction, refreshed.id, submission, &admitted_bases)?;
    if get_assessment_return_tx(&transaction, agent_job_id)?.is_some() {
        return Err(AppError::conflict(
            "assessment_return_already_recorded",
            "this design job already returned for more situation assessment",
        ));
    }
    let revision: i64 = transaction.query_row(
        "SELECT COALESCE(max(revision), 0) + 1 FROM todo_designs WHERE todo_id = ?1",
        [refreshed.todo_id.storage_id()],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO todo_designs(
             todo_id, revision, assessment_id, based_on_design_id, agent_job_id,
             draft_version, state, summary, producer_tool_call_id
         ) VALUES(?1,?2,?3,?4,?5,1,'open',?6,?7)",
        params![
            refreshed.todo_id.storage_id(),
            revision,
            refreshed.id.storage_id(),
            based_on_design_id.map(DesignId::storage_id),
            agent_job_id,
            submission.summary,
            tool_call_id,
        ],
    )?;
    let id = design_id(transaction.last_insert_rowid())?;
    for change in &submission.jurisdiction_changes {
        insert_design_jurisdiction(&transaction, id, change)?;
    }
    for clause in &submission.clauses {
        insert_design_clause(&transaction, id, clause)?;
    }
    for choice in &submission.unresolved_choices {
        insert_design_choice(&transaction, id, choice)?;
    }
    validate_assembled_design(&transaction, id, refreshed.id)?;
    seal_design_if_ready(&transaction, id)?;
    let view = get_design_tx(&transaction, id)?;
    transaction.commit()?;
    Ok(view)
}

pub(crate) fn revise_design(
    connection: &mut Connection,
    id: DesignId,
    revision: &DesignRevision,
) -> AppResult<DesignView> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let before = get_design_tx(&transaction, id)?;
    if before.state != "open" {
        return Err(AppError::conflict(
            "design_not_open",
            format!("design is not open for correction: {id}"),
        ));
    }
    let expected =
        i64::try_from(revision.expected_version).map_err(|_| invalid_number("draft version"))?;
    if expected != before.draft_version {
        return Err(AppError::conflict(
            "design_version_changed",
            format!(
                "design {id} is at draft version {}, not {expected}",
                before.draft_version
            ),
        ));
    }
    if let Some(summary) = &revision.summary {
        transaction.execute(
            "UPDATE todo_designs SET summary = ?2 WHERE id = ?1",
            params![id.storage_id(), summary],
        )?;
    }
    for replacement in &revision.jurisdiction_replacements {
        replace_design_jurisdiction(&transaction, id, replacement)?;
    }
    for addition in &revision.jurisdiction_additions {
        insert_design_jurisdiction(&transaction, id, addition)?;
    }
    for drop in &revision.jurisdiction_drops {
        drop_design_slot(&transaction, "todo_design_jurisdiction_changes", id, drop)?;
    }
    for replacement in &revision.replacements {
        replace_design_clause(&transaction, id, replacement)?;
    }
    for addition in &revision.additions {
        insert_design_clause(&transaction, id, addition)?;
    }
    for drop in &revision.drops {
        drop_design_slot(&transaction, "todo_design_clauses", id, drop)?;
    }
    if let Some(choices) = &revision.unresolved_choices {
        let mut statement = transaction.prepare(
            "SELECT slot FROM todo_design_choices
             WHERE design_id = ?1 AND status = 'active' ORDER BY id",
        )?;
        let active = statement
            .query_map([id.storage_id()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        for operation_id in active {
            record_design_drop(
                &transaction,
                id,
                &operation_id,
                "replaced by a complete unresolved-choice revision",
                &[],
            )?;
            transaction.execute(
                "UPDATE todo_design_choices SET status = 'dropped'
                 WHERE design_id = ?1 AND slot = ?2",
                params![id.storage_id(), operation_id],
            )?;
        }
        for choice in choices {
            insert_design_choice(&transaction, id, choice)?;
        }
    }
    let assessment_id = before
        .assessment_id
        .ok_or_else(|| stored_data_error("open design unexpectedly has no situation assessment"))?;
    let admitted_bases = design_basis_catalog_for_design(&transaction, id)?;
    validate_active_design_basis_refs(&transaction, id, &admitted_bases)?;
    validate_assembled_design(&transaction, id, assessment_id)?;
    transaction.execute(
        "UPDATE todo_designs SET draft_version = draft_version + 1 WHERE id = ?1",
        [id.storage_id()],
    )?;
    seal_design_if_ready(&transaction, id)?;
    let view = get_design_tx(&transaction, id)?;
    transaction.commit()?;
    Ok(view)
}

pub(crate) fn get_design(connection: &Connection, id: DesignId) -> AppResult<DesignView> {
    get_design_tx(connection, id)
}

pub(crate) fn abandon_open_design(
    connection: &mut Connection,
    id: DesignId,
    reason: &str,
) -> AppResult<DesignView> {
    validate_nonblank("design abandonment reason", reason)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let before = get_design_tx(&transaction, id)?;
    if before.state != "open" {
        return Err(AppError::conflict(
            "design_not_open",
            format!("design is not open: {id}"),
        ));
    }
    transaction.execute(
        "UPDATE todo_designs
         SET state = 'abandoned', decision_reason = ?2,
             decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
        params![id.storage_id(), reason],
    )?;
    let view = get_design_tx(&transaction, id)?;
    transaction.commit()?;
    Ok(view)
}

pub(crate) fn record_assessment_return(
    connection: &mut Connection,
    agent_job_id: i64,
    assessment_id: SituationAssessmentId,
    tool_call_id: &str,
    reason: &str,
    missing_or_stale_refs: &[String],
) -> AppResult<AssessmentReturnView> {
    validate_assessment_return_input(tool_call_id, reason, missing_or_stale_refs)?;

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(recorded) = get_assessment_return_tx(&transaction, agent_job_id)? {
        if recorded.assessment_id == assessment_id
            && recorded.reason == reason
            && recorded.producer_tool_call_id == tool_call_id
            && recorded.missing_or_stale_refs == missing_or_stale_refs
        {
            transaction.commit()?;
            return Ok(recorded);
        }
        return Err(AppError::conflict(
            "assessment_return_already_recorded",
            "this design job already recorded a different assessment return",
        ));
    }
    require_assessment_return_subject(&transaction, agent_job_id, assessment_id)?;
    let design_id = abandon_open_design_for_assessment_return(
        &transaction,
        agent_job_id,
        assessment_id,
        reason,
    )?;

    transaction.execute(
        "INSERT INTO todo_design_assessment_returns(
             agent_job_id, assessment_id, design_id, reason, producer_tool_call_id
         ) VALUES(?1,?2,?3,?4,?5)",
        params![
            agent_job_id,
            assessment_id.storage_id(),
            design_id.map(DesignId::storage_id),
            reason,
            tool_call_id,
        ],
    )?;
    for (ordinal, reference) in missing_or_stale_refs.iter().enumerate() {
        transaction.execute(
            "INSERT INTO todo_design_assessment_return_refs(
                 agent_job_id, ordinal, missing_or_stale_ref
             ) VALUES(?1,?2,?3)",
            params![agent_job_id, ordinal_i64(ordinal)?, reference],
        )?;
    }
    let recorded = get_assessment_return_tx(&transaction, agent_job_id)?
        .ok_or_else(|| stored_data_error("assessment return disappeared after it was recorded"))?;
    transaction.commit()?;
    Ok(recorded)
}

fn validate_assessment_return_input(
    tool_call_id: &str,
    reason: &str,
    missing_or_stale_refs: &[String],
) -> AppResult<()> {
    validate_nonblank("tool call ID", tool_call_id)?;
    validate_nonblank("assessment return reason", reason)?;
    if missing_or_stale_refs.is_empty() {
        return Err(AppError::invalid(
            "assessment_return_references_missing",
            "an assessment return must name at least one missing or stale reference",
        ));
    }
    let mut unique_refs = BTreeSet::new();
    for reference in missing_or_stale_refs {
        validate_nonblank("missing or stale reference", reference)?;
        if !unique_refs.insert(reference.as_str()) {
            return Err(AppError::invalid(
                "duplicate_assessment_return_reference",
                format!("assessment return reference is duplicated: {reference}"),
            ));
        }
    }
    Ok(())
}

fn require_assessment_return_subject(
    transaction: &Transaction<'_>,
    agent_job_id: i64,
    assessment_id: SituationAssessmentId,
) -> AppResult<()> {
    let valid: bool = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1
             FROM todo_agent_jobs AS job
             JOIN todo_situation_assessments AS assessment ON assessment.id = ?2
             WHERE job.id = ?1
               AND job.stage = 'design_reconciliation'
               AND job.todo_id = assessment.todo_id
         )",
        params![agent_job_id, assessment_id.storage_id()],
        |row| row.get(0),
    )?;
    if !valid {
        return Err(AppError::conflict(
            "assessment_return_basis_mismatch",
            "the assessment return does not match this design job",
        ));
    }
    Ok(())
}

fn abandon_open_design_for_assessment_return(
    transaction: &Transaction<'_>,
    agent_job_id: i64,
    assessment_id: SituationAssessmentId,
    reason: &str,
) -> AppResult<Option<DesignId>> {
    let design = transaction
        .query_row(
            "SELECT id, assessment_id, state
             FROM todo_designs WHERE agent_job_id = ?1",
            [agent_job_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((raw_id, Some(raw_assessment_id), state)) = design else {
        return match design {
            None => Ok(None),
            Some(_) => Err(assessment_return_too_late()),
        };
    };
    if raw_assessment_id != assessment_id.storage_id() || state != "open" {
        return Err(assessment_return_too_late());
    }
    let id = design_id(raw_id)?;
    transaction.execute(
        "UPDATE todo_designs
         SET state = 'abandoned', decision_reason = ?2,
             decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
        params![id.storage_id(), reason],
    )?;
    Ok(Some(id))
}

fn assessment_return_too_late() -> AppError {
    AppError::conflict(
        "assessment_return_too_late",
        "a situation assessment return cannot replace a ready or terminal design",
    )
}

#[allow(dead_code)] // Durable inspection seam; the current backend retains the returned view.
pub(crate) fn get_assessment_return_for_job(
    connection: &Connection,
    agent_job_id: i64,
) -> AppResult<Option<AssessmentReturnView>> {
    get_assessment_return_tx(connection, agent_job_id)
}

pub(crate) fn discard_design(
    connection: &mut Connection,
    id: DesignId,
    expected_version: u64,
    reason: &str,
) -> AppResult<DesignView> {
    validate_nonblank("design transition reason", reason)?;
    let expected = i64::try_from(expected_version).map_err(|_| invalid_number("draft version"))?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let before = get_design_tx(&transaction, id)?;
    if before.state != "open" {
        return Err(AppError::conflict(
            "design_not_open",
            format!("design is not open: {id}"),
        ));
    }
    if before.draft_version != expected {
        return Err(AppError::conflict(
            "design_version_changed",
            format!("design {id} changed before it could be discarded"),
        ));
    }
    transaction.execute(
        "UPDATE todo_designs
         SET state = 'discarded', decision_reason = ?2,
             decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
        params![id.storage_id(), reason],
    )?;
    let view = get_design_tx(&transaction, id)?;
    transaction.commit()?;
    Ok(view)
}

pub(crate) fn authorize_design(
    connection: &mut Connection,
    id: DesignId,
    source: &DecisionSource,
) -> AppResult<(DesignView, bool)> {
    validate_decision_source(source)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let before = get_design_tx(&transaction, id)?;
    if before.state == "authorized" {
        transaction.commit()?;
        return Ok((before, false));
    }
    if before.state != "ready" {
        return Err(AppError::conflict(
            "design_not_ready",
            format!("design is not ready for acceptance: {id}"),
        ));
    }
    require_canonical_open_todo(&transaction, before.todo_id)?;
    let assessment_id = before.assessment_id.ok_or_else(|| {
        AppError::conflict(
            "design_has_no_assessment",
            "legacy designs cannot be accepted",
        )
    })?;
    let assessment = get_assessment_tx(&transaction, assessment_id)?;
    if !assessment.current {
        transaction.execute(
            "UPDATE todo_designs
             SET state = 'invalidated', decision_reason = ?2,
                 decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
            params![id.storage_id(), "assessment basis changed"],
        )?;
        transaction.commit()?;
        return Err(AppError::conflict(
            "design_basis_changed",
            "the design assessment is no longer current; reassess and propose again",
        ));
    }
    transaction.execute(
        "UPDATE todo_designs
         SET state = 'authorized', decision_source_path = ?2,
             decision_thread_id = ?3, decision_turn_id = ?4,
             decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
        params![
            id.storage_id(),
            source.source_path,
            source.thread_id,
            source.turn_id
        ],
    )?;
    let view = get_design_tx(&transaction, id)?;
    transaction.commit()?;
    Ok((view, true))
}

pub(crate) fn reject_design(
    connection: &mut Connection,
    id: DesignId,
    source: &DecisionSource,
    reason: &str,
) -> AppResult<(DesignView, bool)> {
    validate_decision_source(source)?;
    validate_nonblank("rejection reason", reason)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let before = get_design_tx(&transaction, id)?;
    if before.state == "rejected"
        && before.decision_source_path.as_deref() == Some(source.source_path.as_str())
        && before.decision_reason.as_deref() == Some(reason)
    {
        transaction.commit()?;
        return Ok((before, false));
    }
    if before.state != "ready" {
        return Err(AppError::conflict(
            "design_not_ready",
            format!("design is not ready for rejection: {id}"),
        ));
    }
    transaction.execute(
        "UPDATE todo_designs
         SET state = 'rejected', decision_source_path = ?2,
             decision_thread_id = ?3, decision_turn_id = ?4,
             decision_reason = ?5,
             decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
        params![
            id.storage_id(),
            source.source_path,
            source.thread_id,
            source.turn_id,
            reason
        ],
    )?;
    let view = get_design_tx(&transaction, id)?;
    transaction.commit()?;
    Ok((view, true))
}

fn get_concern_tx(connection: &Connection, id: ConcernId) -> AppResult<Concern> {
    connection
        .query_row(
            "SELECT id, body, source_path, source_thread_id, source_turn_id,
                    source_item_id, status, created_at, resolved_at
             FROM concerns WHERE id = ?1",
            [id.storage_id()],
            concern_from_row,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("concern_not_found", format!("concern not found: {id}")))
}

fn concern_from_row(row: &Row<'_>) -> rusqlite::Result<Concern> {
    let raw_id = row.get::<_, i64>(0)?;
    let status = row.get::<_, String>(6)?;
    Ok(Concern {
        id: ConcernId::from_storage(raw_id).map_err(|error| id_conversion(0, error))?,
        body: row.get(1)?,
        source_path: row.get(2)?,
        source_thread_id: row.get(3)?,
        source_turn_id: row.get(4)?,
        source_item_id: row.get(5)?,
        status: ConcernStatus::parse(&status)?,
        created_at: row.get(7)?,
        resolved_at: row.get(8)?,
    })
}

fn routing_candidates(connection: &Connection) -> AppResult<Vec<RoutingCandidate>> {
    let mut statement = connection.prepare(
        "SELECT t.id, d.id, d.title, d.body, d.revision, t.status
         FROM todos AS t
         JOIN todo_direction_revisions AS d ON d.id = (
             SELECT newest.id FROM todo_direction_revisions AS newest
             WHERE newest.todo_id = t.id
             ORDER BY newest.revision DESC LIMIT 1
         )
         WHERE t.status = 'open'
           AND NOT EXISTS (
             SELECT 1 FROM todo_supersessions AS s
             WHERE s.superseded_todo_id = t.id
         )
         ORDER BY t.id",
    )?;
    let rows = statement.query_map([], |row| {
        let status = row.get::<_, String>(5)?;
        Ok((
            row.get::<_, i64>(1)?,
            RoutingCandidate {
                id: TodoId::from_storage(row.get(0)?).map_err(|error| id_conversion(0, error))?,
                title: row.get(2)?,
                direction: row.get(3)?,
                direction_revision: row.get(4)?,
                status: status
                    .parse()
                    .map_err(|error| id_conversion(5, std::io::Error::other(error)))?,
                boundaries: Vec::new(),
            },
        ))
    })?;
    let mut candidates = rows.collect::<Result<Vec<_>, _>>()?;
    for (direction_id, candidate) in &mut candidates {
        candidate.boundaries = load_boundaries(
            connection,
            "todo_direction_boundaries",
            "direction_revision_id",
            "todo_direction_boundary_sources",
            *direction_id,
        )?;
    }
    Ok(candidates
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect())
}

fn routing_digest(candidates: &[RoutingCandidate]) -> String {
    let mut digest = String::from("routing-heads-v1");
    for candidate in candidates {
        use std::fmt::Write as _;
        let _ = write!(
            digest,
            "|{}:{}:{}",
            candidate.id.storage_id(),
            candidate.direction_revision,
            candidate.status
        );
    }
    digest
}

fn get_routing_tx(
    connection: &Connection,
    id: RoutingProposalId,
) -> AppResult<RoutingProposalView> {
    let mut view = connection
        .query_row(
            "SELECT id, concern_id, action, proposed_title, proposed_direction,
                    rationale, decision, decision_source_path, decision_thread_id,
                    decision_turn_id, decision_reason, decided_at, created_at
             FROM concern_routing_proposals WHERE id = ?1",
            [id.storage_id()],
            |row| {
                Ok(RoutingProposalView {
                    id: RoutingProposalId::from_storage(row.get(0)?)
                        .map_err(|error| id_conversion(0, error))?,
                    concern_id: ConcernId::from_storage(row.get(1)?)
                        .map_err(|error| id_conversion(1, error))?,
                    action: row.get(2)?,
                    targets: Vec::new(),
                    survivor_todo_id: None,
                    proposed_title: row.get(3)?,
                    proposed_direction: row.get(4)?,
                    proposed_boundaries: Vec::new(),
                    rationale: row.get(5)?,
                    evidence_refs: Vec::new(),
                    limitations: Vec::new(),
                    decision: row.get(6)?,
                    decision_source_path: row.get(7)?,
                    decision_thread_id: row.get(8)?,
                    decision_turn_id: row.get(9)?,
                    decision_reason: row.get(10)?,
                    decided_at: row.get(11)?,
                    created_at: row.get(12)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found(
                "routing_not_found",
                format!("routing proposal not found: {id}"),
            )
        })?;
    let mut targets = connection.prepare(
        "SELECT todo_id, direction_revision FROM concern_routing_targets
         WHERE routing_id = ?1 ORDER BY ordinal",
    )?;
    view.targets = targets
        .query_map([id.storage_id()], |row| {
            Ok(RoutingTargetView {
                todo_id: TodoId::from_storage(row.get(0)?)
                    .map_err(|error| id_conversion(0, error))?,
                direction_revision: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    view.survivor_todo_id = connection
        .query_row(
            "SELECT survivor_todo_id FROM concern_routing_unifications
             WHERE routing_id = ?1",
            [id.storage_id()],
            |row| TodoId::from_storage(row.get(0)?).map_err(|error| id_conversion(0, error)),
        )
        .optional()?;
    view.proposed_boundaries = load_routing_boundaries(connection, id)?;
    view.evidence_refs = load_strings(
        connection,
        "concern_routing_evidence",
        "evidence_ref",
        id.storage_id(),
    )?;
    view.limitations = load_strings(
        connection,
        "concern_routing_limitations",
        "limitation",
        id.storage_id(),
    )?;
    Ok(view)
}

fn validate_routing_shape(
    transaction: &Transaction<'_>,
    id: RoutingProposalId,
    action: &str,
) -> AppResult<()> {
    let targets: i64 = transaction.query_row(
        "SELECT count(*) FROM concern_routing_targets WHERE routing_id = ?1",
        [id.storage_id()],
        |row| row.get(0),
    )?;
    let unification: bool = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM concern_routing_unifications WHERE routing_id = ?1
         )",
        [id.storage_id()],
        |row| row.get(0),
    )?;
    let expected_targets = match action {
        "attach" | "revise" => 1,
        "unify" => 2,
        "create" | "dismiss" | "defer" => 0,
        _ => return Err(stored_data_error("invalid routing action")),
    };
    if targets != expected_targets || unification != (action == "unify") {
        return Err(AppError::invalid(
            "invalid_routing_shape",
            format!("routing action {action} has invalid target or unification fields"),
        ));
    }
    if action == "unify" {
        let survivor: i64 = transaction.query_row(
            "SELECT survivor_todo_id FROM concern_routing_unifications
             WHERE routing_id = ?1",
            [id.storage_id()],
            |row| row.get(0),
        )?;
        let selected: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM concern_routing_targets
                 WHERE routing_id = ?1 AND todo_id = ?2
             )",
            params![id.storage_id(), survivor],
            |row| row.get(0),
        )?;
        if !selected {
            return Err(AppError::invalid(
                "invalid_unification_survivor",
                "unification survivor must be one of its two targets",
            ));
        }
    }
    Ok(())
}

fn validate_routing_targets(
    snapshot: &RoutingSnapshot,
    proposal: &ConcernRoutingProposal,
) -> AppResult<()> {
    for target in &proposal.targets {
        let todo_id = parse_todo_id(&target.todo_id)?;
        let candidate = snapshot
            .candidates
            .iter()
            .find(|candidate| candidate.id == todo_id)
            .ok_or_else(|| {
                AppError::invalid(
                    "routing_target_not_in_snapshot",
                    format!("routing target is not in the frozen candidate snapshot: {todo_id}"),
                )
            })?;
        let revision = i64::try_from(target.direction_revision)
            .map_err(|_| invalid_number("direction revision"))?;
        if candidate.direction_revision != revision {
            return Err(AppError::conflict(
                "routing_basis_changed",
                format!(
                    "routing target {todo_id} names direction revision {revision}, but the frozen head is revision {}",
                    candidate.direction_revision
                ),
            ));
        }
        if candidate.status != TodoStatus::Open {
            return Err(AppError::conflict(
                "routing_target_not_open",
                format!("routing target is not open: {todo_id}"),
            ));
        }
    }
    Ok(())
}

fn require_single_target(view: &RoutingProposalView) -> AppResult<&RoutingTargetView> {
    if let [target] = view.targets.as_slice() {
        Ok(target)
    } else {
        Err(stored_data_error(
            "routing proposal has invalid target count",
        ))
    }
}

fn create_todo_from_routing_tx(
    transaction: &Transaction<'_>,
    proposal: &RoutingProposalView,
    routing_id: RoutingProposalId,
) -> AppResult<TodoId> {
    transaction.execute("INSERT INTO todos DEFAULT VALUES", [])?;
    let todo_id = TodoId::from_storage(transaction.last_insert_rowid())
        .map_err(|error| stored_data_error(&format!("invalid todo ID: {error}")))?;
    append_direction_from_routing_tx(transaction, todo_id, proposal, routing_id)?;
    attach_concern_tx(transaction, todo_id, proposal.concern_id, routing_id)?;
    Ok(todo_id)
}

fn append_direction_from_routing_tx(
    transaction: &Transaction<'_>,
    todo_id: TodoId,
    proposal: &RoutingProposalView,
    routing_id: RoutingProposalId,
) -> AppResult<()> {
    let title = proposal
        .proposed_title
        .as_deref()
        .ok_or_else(|| stored_data_error("direction-producing routing proposal has no title"))?;
    let body = proposal.proposed_direction.as_deref().ok_or_else(|| {
        stored_data_error("direction-producing routing proposal has no direction")
    })?;
    let revision: i64 = transaction.query_row(
        "SELECT COALESCE(max(revision), 0) + 1
         FROM todo_direction_revisions WHERE todo_id = ?1",
        [todo_id.storage_id()],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO todo_direction_revisions(
             todo_id, revision, title, body, source_concern_id,
             source_routing_id, provenance_kind
         ) VALUES(?1,?2,?3,?4,?5,?6,'explicit')",
        params![
            todo_id.storage_id(),
            revision,
            title,
            body,
            proposal.concern_id.storage_id(),
            routing_id.storage_id()
        ],
    )?;
    let direction_id = transaction.last_insert_rowid();
    for boundary in &proposal.proposed_boundaries {
        transaction.execute(
            "INSERT INTO todo_direction_boundaries(
                 direction_revision_id, local_ref, kind, statement, attribution
             ) VALUES(?1,?2,?3,?4,?5)",
            params![
                direction_id,
                boundary.local_ref,
                boundary.kind,
                boundary.statement,
                boundary.attribution
            ],
        )?;
        let boundary_id = transaction.last_insert_rowid();
        for (ordinal, source_ref) in boundary.source_refs.iter().enumerate() {
            transaction.execute(
                "INSERT INTO todo_direction_boundary_sources(
                     boundary_id, ordinal, source_ref
                 ) VALUES(?1,?2,?3)",
                params![boundary_id, ordinal_i64(ordinal)?, source_ref],
            )?;
        }
    }
    Ok(())
}

fn attach_concern_tx(
    transaction: &Transaction<'_>,
    todo_id: TodoId,
    concern_id: ConcernId,
    routing_id: RoutingProposalId,
) -> AppResult<()> {
    transaction.execute(
        "INSERT INTO todo_concerns(todo_id, concern_id, authorized_routing_id)
         VALUES(?1,?2,?3)",
        params![
            todo_id.storage_id(),
            concern_id.storage_id(),
            routing_id.storage_id()
        ],
    )?;
    transaction.execute(
        "UPDATE concerns
         SET status = 'attached',
             resolved_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
        [concern_id.storage_id()],
    )?;
    Ok(())
}

fn authorize_unification_tx(
    transaction: &Transaction<'_>,
    proposal: &RoutingProposalView,
    routing_id: RoutingProposalId,
) -> AppResult<TodoId> {
    if proposal.targets.len() != 2 {
        return Err(stored_data_error("unification does not have two targets"));
    }
    let survivor = proposal
        .survivor_todo_id
        .ok_or_else(|| stored_data_error("unification proposal has no survivor"))?;
    let absorbed = proposal
        .targets
        .iter()
        .map(|target| target.todo_id)
        .find(|todo_id| *todo_id != survivor)
        .ok_or_else(|| stored_data_error("unification survivor is not a target"))?;
    for target in &proposal.targets {
        require_exact_direction_revision(transaction, target.todo_id, target.direction_revision)?;
        require_canonical_open_todo(transaction, target.todo_id)?;
    }
    let creates_cycle: bool = transaction.query_row(
        "WITH RECURSIVE successors(id) AS (
             SELECT ?1
             UNION
             SELECT s.surviving_todo_id
             FROM todo_supersessions AS s
             JOIN successors AS prior ON s.superseded_todo_id = prior.id
         )
         SELECT EXISTS(SELECT 1 FROM successors WHERE id = ?2)",
        params![survivor.storage_id(), absorbed.storage_id()],
        |row| row.get(0),
    )?;
    if creates_cycle {
        return Err(AppError::conflict(
            "supersession_cycle",
            "todo unification would create a supersession cycle",
        ));
    }
    append_direction_from_routing_tx(transaction, survivor, proposal, routing_id)?;
    attach_concern_tx(transaction, survivor, proposal.concern_id, routing_id)?;
    transaction.execute(
        "INSERT INTO todo_supersessions(
             superseded_todo_id, surviving_todo_id, authorized_routing_id
         ) VALUES(?1,?2,?3)",
        params![
            absorbed.storage_id(),
            survivor.storage_id(),
            routing_id.storage_id()
        ],
    )?;
    Ok(survivor)
}

fn routed_todo_id(
    connection: &Connection,
    proposal: &RoutingProposalView,
) -> AppResult<Option<TodoId>> {
    match proposal.action.as_str() {
        "attach" | "revise" => Ok(Some(require_single_target(proposal)?.todo_id)),
        "unify" => Ok(proposal.survivor_todo_id),
        "create" => connection
            .query_row(
                "SELECT todo_id FROM todo_concerns WHERE authorized_routing_id = ?1",
                [proposal.id.storage_id()],
                |row| TodoId::from_storage(row.get(0)?).map_err(|error| id_conversion(0, error)),
            )
            .optional()
            .map_err(Into::into),
        "dismiss" | "defer" => Ok(None),
        _ => Err(stored_data_error("invalid routing action")),
    }
}

fn invalidate_routing_tx(
    transaction: &Transaction<'_>,
    id: RoutingProposalId,
    reason: &str,
) -> AppResult<()> {
    transaction.execute(
        "UPDATE concern_routing_proposals
         SET decision = 'invalidated', decision_reason = ?2,
             decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
        params![id.storage_id(), reason],
    )?;
    Ok(())
}

fn current_direction(connection: &Connection, todo_id: TodoId) -> AppResult<DirectionRevision> {
    let mut direction = connection
        .query_row(
            "SELECT id, todo_id, revision, title, body, provenance_kind, created_at
             FROM todo_direction_revisions
             WHERE todo_id = ?1 ORDER BY revision DESC LIMIT 1",
            [todo_id.storage_id()],
            |row| {
                Ok(DirectionRevision {
                    id: row.get(0)?,
                    todo_id: TodoId::from_storage(row.get(1)?)
                        .map_err(|error| id_conversion(1, error))?,
                    revision: row.get(2)?,
                    title: row.get(3)?,
                    body: row.get(4)?,
                    provenance_kind: row.get(5)?,
                    created_at: row.get(6)?,
                    boundaries: Vec::new(),
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found(
                "todo_not_found",
                format!("todo has no direction: {todo_id}"),
            )
        })?;
    direction.boundaries = load_direction_boundaries(connection, direction.id)?;
    Ok(direction)
}

fn load_direction_boundaries(
    connection: &Connection,
    direction_id: i64,
) -> AppResult<Vec<DirectionBoundary>> {
    load_boundaries(
        connection,
        "todo_direction_boundaries",
        "direction_revision_id",
        "todo_direction_boundary_sources",
        direction_id,
    )
}

fn load_routing_boundaries(
    connection: &Connection,
    routing_id: RoutingProposalId,
) -> AppResult<Vec<DirectionBoundary>> {
    load_boundaries(
        connection,
        "concern_routing_boundaries",
        "routing_id",
        "concern_routing_boundary_sources",
        routing_id.storage_id(),
    )
}

fn load_boundaries(
    connection: &Connection,
    table: &str,
    parent_column: &str,
    sources_table: &str,
    parent_id: i64,
) -> AppResult<Vec<DirectionBoundary>> {
    let sql = format!(
        "SELECT id, local_ref, kind, statement, attribution
         FROM {table} WHERE {parent_column} = ?1 ORDER BY id"
    );
    let mut statement = connection.prepare(&sql)?;
    let mut boundaries = statement
        .query_map([parent_id], |row| {
            Ok(DirectionBoundary {
                id: row.get(0)?,
                local_ref: row.get(1)?,
                kind: row.get(2)?,
                statement: row.get(3)?,
                attribution: row.get(4)?,
                source_refs: Vec::new(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for boundary in &mut boundaries {
        boundary.source_refs = load_child_strings(
            connection,
            sources_table,
            "boundary_id",
            "source_ref",
            boundary.id,
        )?;
    }
    Ok(boundaries)
}

fn effective_family(connection: &Connection, todo_id: TodoId) -> AppResult<Vec<i64>> {
    let mut statement = connection.prepare(
        "WITH RECURSIVE family(id) AS (
             SELECT ?1
             UNION
             SELECT s.superseded_todo_id
             FROM todo_supersessions AS s
             JOIN family AS current ON s.surviving_todo_id = current.id
         )
         SELECT id FROM family ORDER BY id",
    )?;
    let values = statement
        .query_map([todo_id.storage_id()], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(values)
}

pub(crate) fn effective_concerns(
    connection: &Connection,
    todo_id: TodoId,
) -> AppResult<Vec<Concern>> {
    let family = effective_family(connection, todo_id)?;
    let mut concerns = Vec::new();
    for member in family {
        let mut statement = connection.prepare(
            "SELECT c.id, c.body, c.source_path, c.source_thread_id,
                    c.source_turn_id, c.source_item_id, c.status,
                    c.created_at, c.resolved_at
             FROM todo_concerns AS link
             JOIN concerns AS c ON c.id = link.concern_id
             WHERE link.todo_id = ?1 ORDER BY link.id",
        )?;
        concerns.extend(
            statement
                .query_map([member], concern_from_row)?
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    concerns.sort_by_key(|concern| concern.id);
    Ok(concerns)
}

fn concern_set_digest(connection: &Connection, todo_id: TodoId) -> AppResult<String> {
    let concerns = effective_concerns(connection, todo_id)?;
    Ok(format!(
        "concerns-v1:{}",
        concerns
            .iter()
            .map(|concern| concern.id.storage_id().to_string())
            .collect::<Vec<_>>()
            .join(",")
    ))
}

fn effective_note_cursor(connection: &Connection, todo_id: TodoId) -> AppResult<Option<i64>> {
    let family = effective_family(connection, todo_id)?;
    let mut cursor = None;
    for member in family {
        let value: Option<i64> = connection.query_row(
            "SELECT max(id) FROM todo_notes WHERE todo_id = ?1",
            [member],
            |row| row.get(0),
        )?;
        cursor = match (cursor, value) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (None, value) | (value, None) => value,
        };
    }
    Ok(cursor)
}

fn latest_authorized_design_id(
    connection: &Connection,
    todo_id: TodoId,
) -> AppResult<Option<DesignId>> {
    connection
        .query_row(
            "SELECT id FROM todo_designs
             WHERE todo_id = ?1 AND state = 'authorized'
             ORDER BY revision DESC LIMIT 1",
            [todo_id.storage_id()],
            |row| DesignId::from_storage(row.get(0)?).map_err(|error| id_conversion(0, error)),
        )
        .optional()
        .map_err(Into::into)
}

fn get_assessment_tx(
    connection: &Connection,
    id: SituationAssessmentId,
) -> AppResult<SituationAssessmentView> {
    let mut view = connection
        .query_row(
            "SELECT a.id, a.todo_id, a.direction_revision_id, d.revision,
                    a.concern_set_digest, a.notes_through_id, a.based_on_design_id,
                    a.disposition, a.summary, a.subject_label, a.observed_at
             FROM todo_situation_assessments AS a
             JOIN todo_direction_revisions AS d ON d.id = a.direction_revision_id
             WHERE a.id = ?1",
            [id.storage_id()],
            |row| {
                let note = row.get::<_, Option<i64>>(5)?;
                let based_on = row.get::<_, Option<i64>>(6)?;
                Ok(SituationAssessmentView {
                    id: SituationAssessmentId::from_storage(row.get(0)?)
                        .map_err(|error| id_conversion(0, error))?,
                    todo_id: TodoId::from_storage(row.get(1)?)
                        .map_err(|error| id_conversion(1, error))?,
                    direction_revision: row.get(3)?,
                    direction_revision_id: row.get(2)?,
                    concern_set_digest: row.get(4)?,
                    notes_through_id: note
                        .map(|value| {
                            WorkingNoteId::from_storage(value)
                                .map_err(|error| id_conversion(5, error))
                        })
                        .transpose()?,
                    based_on_design_id: based_on
                        .map(|value| {
                            DesignId::from_storage(value).map_err(|error| id_conversion(6, error))
                        })
                        .transpose()?,
                    disposition: row.get(7)?,
                    summary: row.get(8)?,
                    subject_label: row.get(9)?,
                    observed_at: row.get(10)?,
                    current: false,
                    stale_reasons: Vec::new(),
                    bases: Vec::new(),
                    identity_refs: Vec::new(),
                    findings: Vec::new(),
                    jurisdictions: Vec::new(),
                    direction_mappings: Vec::new(),
                    unresolved: Vec::new(),
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found(
                "assessment_not_found",
                format!("situation assessment not found: {id}"),
            )
        })?;
    view.bases = load_assessment_bases(connection, id)?;
    view.identity_refs = load_child_strings(
        connection,
        "todo_assessment_identity_refs",
        "assessment_id",
        "identity_ref",
        id.storage_id(),
    )?;
    view.findings = load_assessment_findings(connection, id)?;
    view.jurisdictions = load_assessment_jurisdictions(connection, id)?;
    view.direction_mappings = load_assessment_direction_mappings(connection, id)?;
    view.unresolved = load_assessment_unresolved(connection, id)?;
    view.stale_reasons = assessment_stale_reasons(connection, &view)?;
    view.current = view.stale_reasons.is_empty();
    Ok(view)
}

fn assessment_stale_reasons(
    connection: &Connection,
    assessment: &SituationAssessmentView,
) -> AppResult<Vec<String>> {
    let mut reasons = Vec::new();
    let newer_exists: bool = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM todo_situation_assessments
             WHERE todo_id = ?1 AND id > ?2
         )",
        params![assessment.todo_id.storage_id(), assessment.id.storage_id()],
        |row| row.get(0),
    )?;
    if newer_exists {
        reasons.push("newer situation assessment exists".to_owned());
    }
    let direction = current_direction(connection, assessment.todo_id)?;
    if direction.id != assessment.direction_revision_id {
        reasons.push("direction revision changed".to_owned());
    }
    if concern_set_digest(connection, assessment.todo_id)? != assessment.concern_set_digest {
        reasons.push("effective concern set changed".to_owned());
    }
    if effective_note_cursor(connection, assessment.todo_id)?
        != assessment.notes_through_id.map(WorkingNoteId::storage_id)
    {
        reasons.push("working-note cursor changed".to_owned());
    }
    let current_design = latest_authorized_design_id(connection, assessment.todo_id)?;
    if current_design != assessment.based_on_design_id {
        let derived_from_this_assessment = match current_design {
            Some(design_id) => connection.query_row(
                "SELECT assessment_id = ?2 FROM todo_designs WHERE id = ?1",
                params![design_id.storage_id(), assessment.id.storage_id()],
                |row| row.get::<_, bool>(0),
            )?,
            None => false,
        };
        if !derived_from_this_assessment {
            reasons.push("accepted-design basis changed".to_owned());
        }
    }
    Ok(reasons)
}

fn load_assessment_bases(
    connection: &Connection,
    id: SituationAssessmentId,
) -> AppResult<Vec<AssessmentBase>> {
    let mut statement = connection.prepare(
        "SELECT source_ref, kind, locator, revision, observed_at
         FROM todo_assessment_bases WHERE assessment_id = ?1 ORDER BY id",
    )?;
    let rows = statement.query_map([id.storage_id()], |row| {
        Ok(AssessmentBase {
            source_ref: row.get(0)?,
            kind: row.get(1)?,
            locator: row.get(2)?,
            revision: row.get(3)?,
            observed_at: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn load_assessment_findings(
    connection: &Connection,
    id: SituationAssessmentId,
) -> AppResult<Vec<serde_json::Value>> {
    let mut statement = connection.prepare(
        "SELECT id, local_ref, kind, claim
         FROM todo_assessment_findings WHERE assessment_id = ?1 ORDER BY id",
    )?;
    let rows = statement
        .query_map([id.storage_id()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(row_id, local_ref, kind, claim)| {
            let evidence_refs = load_child_strings(
                connection,
                "todo_assessment_finding_evidence",
                "finding_id",
                "evidence_ref",
                row_id,
            )?;
            Ok(serde_json::json!({
                "ref": local_ref,
                "kind": kind,
                "claim": claim,
                "evidence_refs": evidence_refs,
            }))
        })
        .collect()
}

fn load_assessment_jurisdictions(
    connection: &Connection,
    id: SituationAssessmentId,
) -> AppResult<Vec<serde_json::Value>> {
    let mut statement = connection.prepare(
        "SELECT id, jurisdiction_key, concern
         FROM todo_assessment_jurisdictions WHERE assessment_id = ?1 ORDER BY id",
    )?;
    let rows = statement
        .query_map([id.storage_id()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(row_id, key, concern)| {
            let mut assignments_statement = connection.prepare(
                "SELECT party, role, responsibility
                 FROM todo_assessment_jurisdiction_assignments
                 WHERE jurisdiction_id = ?1 ORDER BY party, role",
            )?;
            let assignments = assignments_statement
                .query_map([row_id], |row| {
                    Ok(serde_json::json!({
                        "party": row.get::<_, String>(0)?,
                        "role": row.get::<_, String>(1)?,
                        "responsibility": row.get::<_, String>(2)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let evidence_refs = load_child_strings(
                connection,
                "todo_assessment_jurisdiction_evidence",
                "jurisdiction_id",
                "evidence_ref",
                row_id,
            )?;
            Ok(serde_json::json!({
                "key": key,
                "concern": concern,
                "assignments": assignments,
                "evidence_refs": evidence_refs,
            }))
        })
        .collect()
}

fn load_assessment_direction_mappings(
    connection: &Connection,
    id: SituationAssessmentId,
) -> AppResult<Vec<serde_json::Value>> {
    let mut statement = connection.prepare(
        "SELECT m.id, b.local_ref, m.disposition, m.explanation
         FROM todo_assessment_direction_mappings AS m
         JOIN todo_direction_boundaries AS b ON b.id = m.boundary_id
         WHERE m.assessment_id = ?1 ORDER BY m.id",
    )?;
    let rows = statement
        .query_map([id.storage_id()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(mapping_id, boundary_ref, disposition, explanation)| {
            let mut finding_statement = connection.prepare(
                "SELECT f.local_ref
                 FROM todo_assessment_mapping_findings AS link
                 JOIN todo_assessment_findings AS f ON f.id = link.finding_id
                 WHERE link.mapping_id = ?1 ORDER BY f.id",
            )?;
            let finding_refs = finding_statement
                .query_map([mapping_id], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            Ok(serde_json::json!({
                "boundary_ref": boundary_ref,
                "disposition": disposition,
                "finding_refs": finding_refs,
                "explanation": explanation,
            }))
        })
        .collect()
}

fn load_assessment_unresolved(
    connection: &Connection,
    id: SituationAssessmentId,
) -> AppResult<Vec<serde_json::Value>> {
    let mut statement = connection.prepare(
        "SELECT id, local_ref, kind, description, materiality
         FROM todo_assessment_unresolved WHERE assessment_id = ?1 ORDER BY id",
    )?;
    let rows = statement
        .query_map([id.storage_id()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(row_id, local_ref, kind, description, materiality)| {
            let evidence_refs = load_child_strings(
                connection,
                "todo_assessment_unresolved_evidence",
                "unresolved_id",
                "evidence_ref",
                row_id,
            )?;
            Ok(serde_json::json!({
                "ref": local_ref,
                "kind": kind,
                "description": description,
                "materiality": materiality,
                "evidence_refs": evidence_refs,
            }))
        })
        .collect()
}

fn validate_assessment_refs(
    snapshot: &AssessmentSnapshot,
    assessment: &SituationAssessment,
) -> AppResult<()> {
    if assessment.disposition == AssessmentDisposition::Ready && !assessment.unresolved.is_empty() {
        return Err(AppError::invalid(
            "ready_assessment_has_unresolved_items",
            "a ready assessment cannot retain unresolved items",
        ));
    }
    let expected = snapshot
        .direction
        .boundaries
        .iter()
        .map(|boundary| boundary.local_ref.as_str())
        .collect::<BTreeSet<_>>();
    let mapped = assessment
        .direction_mappings
        .iter()
        .map(|mapping| mapping.boundary_ref.as_str())
        .collect::<BTreeSet<_>>();
    if expected != mapped || assessment.direction_mappings.len() != expected.len() {
        return Err(AppError::invalid(
            "incomplete_direction_mapping",
            "assessment must map every direction boundary exactly once",
        ));
    }
    let findings = assessment
        .findings
        .iter()
        .map(|finding| finding.r#ref.as_str())
        .collect::<BTreeSet<_>>();
    for mapping in &assessment.direction_mappings {
        if mapping
            .finding_refs
            .iter()
            .any(|reference| !findings.contains(reference.as_str()))
        {
            return Err(AppError::invalid(
                "unknown_finding_reference",
                "direction mapping references an unknown finding",
            ));
        }
    }
    Ok(())
}

fn validate_assessment_source_refs(
    assessment: &SituationAssessment,
    bases: &[AssessmentBase],
) -> AppResult<()> {
    let base_refs = bases
        .iter()
        .map(|base| base.source_ref.as_str())
        .collect::<BTreeSet<_>>();
    if base_refs.len() != bases.len() {
        return Err(AppError::invalid(
            "duplicate_assessment_source_ref",
            "assessment bases must have unique source refs",
        ));
    }
    let evidence_refs = assessment
        .subject
        .identity_refs
        .iter()
        .chain(
            assessment
                .findings
                .iter()
                .flat_map(|finding| finding.evidence_refs.iter()),
        )
        .chain(
            assessment
                .jurisdictions
                .iter()
                .flat_map(|jurisdiction| jurisdiction.evidence_refs.iter()),
        )
        .chain(
            assessment
                .unresolved
                .iter()
                .flat_map(|unresolved| unresolved.evidence_refs.iter()),
        );
    let missing = evidence_refs
        .filter_map(|reference| {
            reference
                .strip_prefix("source:")
                .and_then(|_| reference.find('@').map(|separator| &reference[..separator]))
        })
        .filter(|source_ref| !base_refs.contains(*source_ref))
        .collect::<BTreeSet<_>>();
    if !missing.is_empty() {
        return Err(AppError::invalid(
            "unknown_assessment_source_ref",
            format!(
                "assessment evidence references sources absent from its bases: {}",
                missing.into_iter().collect::<Vec<_>>().join(", ")
            ),
        ));
    }
    Ok(())
}

fn get_assessment_return_tx(
    connection: &Connection,
    agent_job_id: i64,
) -> AppResult<Option<AssessmentReturnView>> {
    let mut view = connection
        .query_row(
            "SELECT assessment_id, design_id, reason, producer_tool_call_id, created_at
             FROM todo_design_assessment_returns WHERE agent_job_id = ?1",
            [agent_job_id],
            |row| {
                let design = row.get::<_, Option<i64>>(1)?;
                Ok(AssessmentReturnView {
                    assessment_id: SituationAssessmentId::from_storage(row.get(0)?)
                        .map_err(|error| id_conversion(0, error))?,
                    design_id: design
                        .map(|value| {
                            DesignId::from_storage(value).map_err(|error| id_conversion(1, error))
                        })
                        .transpose()?,
                    reason: row.get(2)?,
                    missing_or_stale_refs: Vec::new(),
                    producer_tool_call_id: row.get(3)?,
                    created_at: row.get(4)?,
                })
            },
        )
        .optional()?;
    if let Some(recorded) = &mut view {
        let mut statement = connection.prepare(
            "SELECT missing_or_stale_ref
             FROM todo_design_assessment_return_refs
             WHERE agent_job_id = ?1 ORDER BY ordinal",
        )?;
        recorded.missing_or_stale_refs = statement
            .query_map([agent_job_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
    }
    Ok(view)
}

fn get_design_correction_tx(
    connection: &Connection,
    agent_job_id: i64,
) -> AppResult<Option<DesignCorrection>> {
    connection
        .query_row(
            "SELECT agent_job_id, based_on_design_id, feedback, basis_ref, created_at
             FROM todo_design_corrections WHERE agent_job_id = ?1",
            [agent_job_id],
            |row| {
                Ok(DesignCorrection {
                    agent_job_id: row.get(0)?,
                    based_on_design_id: DesignId::from_storage(row.get(1)?)
                        .map_err(|error| id_conversion(1, error))?,
                    feedback: row.get(2)?,
                    basis_ref: row.get(3)?,
                    created_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn get_design_tx(connection: &Connection, id: DesignId) -> AppResult<DesignView> {
    let mut view = connection
        .query_row(
            "SELECT id, todo_id, revision, assessment_id, based_on_design_id,
                    draft_version, state, summary, decision_source_path,
                    decision_thread_id, decision_turn_id, decision_reason,
                    decided_at, created_at
             FROM todo_designs WHERE id = ?1",
            [id.storage_id()],
            |row| {
                let assessment = row.get::<_, Option<i64>>(3)?;
                let based_on = row.get::<_, Option<i64>>(4)?;
                Ok(DesignView {
                    id: DesignId::from_storage(row.get(0)?)
                        .map_err(|error| id_conversion(0, error))?,
                    todo_id: TodoId::from_storage(row.get(1)?)
                        .map_err(|error| id_conversion(1, error))?,
                    revision: row.get(2)?,
                    assessment_id: assessment
                        .map(|value| {
                            SituationAssessmentId::from_storage(value)
                                .map_err(|error| id_conversion(3, error))
                        })
                        .transpose()?,
                    based_on_design_id: based_on
                        .map(|value| {
                            DesignId::from_storage(value).map_err(|error| id_conversion(4, error))
                        })
                        .transpose()?,
                    draft_version: row.get(5)?,
                    state: row.get(6)?,
                    summary: row.get(7)?,
                    current: false,
                    stale_reasons: Vec::new(),
                    jurisdiction_changes: Vec::new(),
                    clauses: Vec::new(),
                    unresolved_choices: Vec::new(),
                    correction_basis_ref: None,
                    correction_feedback: None,
                    decision_source_path: row.get(8)?,
                    decision_thread_id: row.get(9)?,
                    decision_turn_id: row.get(10)?,
                    decision_reason: row.get(11)?,
                    decided_at: row.get(12)?,
                    created_at: row.get(13)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found("design_not_found", format!("design not found: {id}"))
        })?;
    let agent_job_id = connection.query_row(
        "SELECT agent_job_id FROM todo_designs WHERE id = ?1",
        [id.storage_id()],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    if let Some(correction) = agent_job_id
        .map(|agent_job_id| get_design_correction_tx(connection, agent_job_id))
        .transpose()?
        .flatten()
    {
        view.correction_basis_ref = Some(correction.basis_ref);
        view.correction_feedback = Some(correction.feedback);
    }
    view.jurisdiction_changes = load_design_jurisdictions(connection, id)?;
    view.clauses = load_design_clauses(connection, id)?;
    view.unresolved_choices = load_design_choices(connection, id)?;
    if let Some(assessment_id) = view.assessment_id {
        let assessment = get_assessment_tx(connection, assessment_id)?;
        if !assessment.current {
            view.stale_reasons
                .push("situation assessment is no longer current".to_owned());
        }
    } else if view.state == "legacy_unreviewed" {
        view.stale_reasons
            .push("legacy design has not been assessed".to_owned());
    }
    view.current = view.stale_reasons.is_empty();
    Ok(view)
}

fn load_design_jurisdictions(
    connection: &Connection,
    id: DesignId,
) -> AppResult<Vec<serde_json::Value>> {
    let mut statement = connection.prepare(
        "SELECT id, slot, local_ref, jurisdiction_key, action, rationale, status
         FROM todo_design_jurisdiction_changes
         WHERE design_id = ?1 ORDER BY id",
    )?;
    let rows = statement
        .query_map([id.storage_id()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(row_id, slot, local_ref, key, action, rationale, status)| {
                let expected = load_design_assignments(connection, row_id, "expected")?;
                let proposed = load_design_assignments(connection, row_id, "proposed")?;
                let basis_refs = load_child_strings(
                    connection,
                    "todo_design_jurisdiction_bases",
                    "jurisdiction_change_id",
                    "basis_ref",
                    row_id,
                )?;
                let drop = load_design_drop(connection, id, &slot)?;
                Ok(serde_json::json!({
                    "operation_id": slot,
                    "local_ref": local_ref,
                    "key": key,
                    "action": action,
                    "rationale": rationale,
                    "status": status,
                    "expected_assignments": expected,
                    "proposed_assignments": proposed,
                    "basis_refs": basis_refs,
                    "drop": drop,
                }))
            },
        )
        .collect()
}

fn load_design_assignments(
    connection: &Connection,
    change_id: i64,
    side: &str,
) -> AppResult<Vec<serde_json::Value>> {
    let mut statement = connection.prepare(
        "SELECT party, role, responsibility
         FROM todo_design_responsibilities
         WHERE jurisdiction_change_id = ?1 AND side = ?2
         ORDER BY party, role",
    )?;
    let rows = statement.query_map(params![change_id, side], |row| {
        Ok(serde_json::json!({
            "party": row.get::<_, String>(0)?,
            "role": row.get::<_, String>(1)?,
            "responsibility": row.get::<_, String>(2)?,
        }))
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn load_design_clauses(connection: &Connection, id: DesignId) -> AppResult<Vec<serde_json::Value>> {
    let mut statement = connection.prepare(
        "SELECT id, slot, local_ref, kind, subject, statement, jurisdiction_key, status
         FROM todo_design_clauses WHERE design_id = ?1 ORDER BY id",
    )?;
    let rows = statement
        .query_map([id.storage_id()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(row_id, slot, local_ref, kind, subject, statement, jurisdiction, status)| {
                let basis_refs = load_child_strings(
                    connection,
                    "todo_design_clause_bases",
                    "clause_id",
                    "basis_ref",
                    row_id,
                )?;
                let drop = load_design_drop(connection, id, &slot)?;
                Ok(serde_json::json!({
                    "operation_id": slot,
                    "local_ref": local_ref,
                    "kind": kind,
                    "subject": subject,
                    "statement": statement,
                    "jurisdiction_ref": jurisdiction,
                    "status": status,
                    "basis_refs": basis_refs,
                    "drop": drop,
                }))
            },
        )
        .collect()
}

fn load_design_choices(connection: &Connection, id: DesignId) -> AppResult<Vec<serde_json::Value>> {
    let mut statement = connection.prepare(
        "SELECT id, slot, local_ref, question, materiality, status
         FROM todo_design_choices WHERE design_id = ?1 ORDER BY id",
    )?;
    let rows = statement
        .query_map([id.storage_id()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(row_id, slot, local_ref, question, materiality, status)| {
            let basis_refs = load_child_strings(
                connection,
                "todo_design_choice_bases",
                "choice_id",
                "basis_ref",
                row_id,
            )?;
            let drop = load_design_drop(connection, id, &slot)?;
            Ok(serde_json::json!({
                "operation_id": slot,
                "local_ref": local_ref,
                "question": question,
                "why_material": materiality,
                "status": status,
                "basis_refs": basis_refs,
                "drop": drop,
            }))
        })
        .collect()
}

fn load_design_drop(
    connection: &Connection,
    id: DesignId,
    operation_id: &str,
) -> AppResult<Option<serde_json::Value>> {
    let row = connection
        .query_row(
            "SELECT reason, dropped_at FROM todo_design_operation_drops
             WHERE design_id = ?1 AND operation_id = ?2",
            params![id.storage_id(), operation_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    row.map(|(reason, dropped_at)| {
        // This table has a composite parent key, so load its short bounded list directly.
        let mut statement = connection.prepare(
            "SELECT basis_ref FROM todo_design_operation_drop_bases
             WHERE design_id = ?1 AND operation_id = ?2 ORDER BY ordinal",
        )?;
        let basis_refs = statement
            .query_map(params![id.storage_id(), operation_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(serde_json::json!({
            "reason": reason,
            "basis_refs": basis_refs,
            "dropped_at": dropped_at,
        }))
    })
    .transpose()
}

fn design_basis_catalog(
    connection: &Connection,
    assessment_id: SituationAssessmentId,
    based_on_design_id: Option<DesignId>,
    agent_job_id: i64,
) -> AppResult<BTreeSet<String>> {
    let mut catalog = BTreeSet::from([
        "direction:body".to_owned(),
        format!("assessment:{assessment_id}"),
    ]);
    let direction_revision_id: i64 = connection.query_row(
        "SELECT direction_revision_id FROM todo_situation_assessments WHERE id = ?1",
        [assessment_id.storage_id()],
        |row| row.get(0),
    )?;
    let mut statement = connection.prepare(
        "SELECT local_ref FROM todo_direction_boundaries
         WHERE direction_revision_id = ?1 ORDER BY id",
    )?;
    catalog.extend(
        statement
            .query_map([direction_revision_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|local_ref| format!("direction:{local_ref}")),
    );
    let mut statement = connection.prepare(
        "SELECT local_ref FROM todo_assessment_findings
         WHERE assessment_id = ?1 ORDER BY id",
    )?;
    catalog.extend(
        statement
            .query_map([assessment_id.storage_id()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|local_ref| format!("assessment:{assessment_id}:finding:{local_ref}")),
    );
    let mut statement = connection.prepare(
        "SELECT jurisdiction_key FROM todo_assessment_jurisdictions
         WHERE assessment_id = ?1 ORDER BY id",
    )?;
    catalog.extend(
        statement
            .query_map([assessment_id.storage_id()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|key| format!("assessment:{assessment_id}:jurisdiction:{key}")),
    );

    if let Some(predecessor) = based_on_design_id {
        let mut statement = connection.prepare(
            "SELECT slot FROM todo_design_jurisdiction_changes
             WHERE design_id = ?1 AND status = 'active'
             UNION
             SELECT slot FROM todo_design_clauses
             WHERE design_id = ?1 AND status = 'active'
             UNION
             SELECT slot FROM todo_design_choices
             WHERE design_id = ?1 AND status = 'active'",
        )?;
        catalog.extend(
            statement
                .query_map([predecessor.storage_id()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|operation| format!("design:{predecessor}:{operation}")),
        );
    }

    let correction = get_design_correction_tx(connection, agent_job_id)?;
    if let Some(correction) = correction {
        if Some(correction.based_on_design_id) != based_on_design_id {
            return Err(AppError::conflict(
                "design_correction_basis_mismatch",
                "the correction record does not match the exact based-on design",
            ));
        }
        catalog.insert(correction.basis_ref);
    } else if let Some(predecessor) = based_on_design_id {
        let state: String = connection.query_row(
            "SELECT state FROM todo_designs WHERE id = ?1",
            [predecessor.storage_id()],
            |row| row.get(0),
        )?;
        if matches!(state.as_str(), "ready" | "rejected" | "abandoned") {
            return Err(AppError::conflict(
                "design_correction_missing",
                "a correction run must record its exact feedback before creating or revising a design",
            ));
        }
    }
    Ok(catalog)
}

fn design_basis_catalog_for_design(
    connection: &Connection,
    id: DesignId,
) -> AppResult<BTreeSet<String>> {
    let (raw_assessment_id, raw_based_on_design_id, agent_job_id) = connection.query_row(
        "SELECT assessment_id, based_on_design_id, agent_job_id
         FROM todo_designs WHERE id = ?1",
        [id.storage_id()],
        |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        },
    )?;
    let assessment_id = raw_assessment_id
        .map(assessment_id)
        .transpose()?
        .ok_or_else(|| stored_data_error("design has no assessment basis"))?;
    let based_on_design_id = raw_based_on_design_id.map(design_id).transpose()?;
    let agent_job_id = agent_job_id
        .ok_or_else(|| stored_data_error("design has no design-reconciliation agent job"))?;
    design_basis_catalog(connection, assessment_id, based_on_design_id, agent_job_id)
}

fn validate_design_basis_refs<'a>(
    admitted: &BTreeSet<String>,
    references: impl IntoIterator<Item = &'a String>,
) -> AppResult<()> {
    let unknown = references
        .into_iter()
        .filter(|reference| !admitted.contains(reference.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>();
    if !unknown.is_empty() {
        return Err(AppError::invalid(
            "unknown_design_basis_reference",
            format!(
                "design operation cites basis refs outside its admitted catalog: {}",
                unknown.into_iter().collect::<Vec<_>>().join(", ")
            ),
        ));
    }
    Ok(())
}

fn active_design_basis_refs(connection: &Connection, id: DesignId) -> AppResult<BTreeSet<String>> {
    let mut statement = connection.prepare(
        "SELECT bases.basis_ref
         FROM todo_design_jurisdiction_bases AS bases
         JOIN todo_design_jurisdiction_changes AS operation
           ON operation.id = bases.jurisdiction_change_id
         WHERE operation.design_id = ?1 AND operation.status = 'active'
         UNION
         SELECT bases.basis_ref
         FROM todo_design_clause_bases AS bases
         JOIN todo_design_clauses AS operation ON operation.id = bases.clause_id
         WHERE operation.design_id = ?1 AND operation.status = 'active'
         UNION
         SELECT bases.basis_ref
         FROM todo_design_choice_bases AS bases
         JOIN todo_design_choices AS operation ON operation.id = bases.choice_id
         WHERE operation.design_id = ?1 AND operation.status = 'active'",
    )?;
    statement
        .query_map([id.storage_id()], |row| row.get::<_, String>(0))?
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(Into::into)
}

fn validate_active_design_basis_refs(
    connection: &Connection,
    id: DesignId,
    admitted: &BTreeSet<String>,
) -> AppResult<()> {
    let active = active_design_basis_refs(connection, id)?;
    validate_design_basis_refs(admitted, &active)
}

fn validate_design_refs(
    connection: &Connection,
    assessment_id: SituationAssessmentId,
    submission: &DesignSubmission,
    admitted_bases: &BTreeSet<String>,
) -> AppResult<()> {
    let jurisdictions = assessment_jurisdiction_assignments(connection, assessment_id)?;
    let mut seen = BTreeSet::new();
    for change in &submission.jurisdiction_changes {
        if !seen.insert(change.key.as_str()) {
            return Err(AppError::invalid(
                "duplicate_jurisdiction_change",
                format!("jurisdiction is changed more than once: {}", change.key),
            ));
        }
        if change.action == JurisdictionAction::Add {
            if jurisdictions.contains_key(&change.key) {
                return Err(AppError::invalid(
                    "jurisdiction_already_exists",
                    format!(
                        "assessed jurisdiction cannot be added again: {}",
                        change.key
                    ),
                ));
            }
        } else {
            let expected = jurisdictions.get(&change.key).ok_or_else(|| {
                AppError::invalid(
                    "unknown_jurisdiction_reference",
                    format!("unknown assessed jurisdiction: {}", change.key),
                )
            })?;
            let supplied = canonical_assignments(&change.expected_assignments);
            if &supplied != expected {
                return Err(AppError::conflict(
                    "jurisdiction_basis_changed",
                    format!(
                        "expected assignments do not exactly match assessment jurisdiction {}",
                        change.key
                    ),
                ));
            }
        }
    }
    let missing = jurisdictions
        .keys()
        .filter(|key| !seen.contains(key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(AppError::invalid(
            "incomplete_jurisdiction_mapping",
            format!(
                "design must map every assessed jurisdiction exactly once; missing: {}",
                missing.join(", ")
            ),
        ));
    }
    let active = submission
        .jurisdiction_changes
        .iter()
        .filter(|change| change.action != JurisdictionAction::Retire)
        .map(|change| change.key.as_str())
        .collect::<BTreeSet<_>>();
    for clause in &submission.clauses {
        if let Some(reference) = &clause.jurisdiction_ref
            && !active.contains(reference.as_str())
        {
            return Err(AppError::invalid(
                "unknown_jurisdiction_reference",
                format!("unknown jurisdiction in design clause: {reference}"),
            ));
        }
    }
    validate_design_basis_refs(
        admitted_bases,
        submission
            .jurisdiction_changes
            .iter()
            .flat_map(|change| change.basis_refs.iter())
            .chain(
                submission
                    .clauses
                    .iter()
                    .flat_map(|clause| clause.basis_refs.iter()),
            )
            .chain(
                submission
                    .unresolved_choices
                    .iter()
                    .flat_map(|choice| choice.basis_refs.iter()),
            ),
    )
}

fn assessment_jurisdiction_assignments(
    connection: &Connection,
    assessment_id: SituationAssessmentId,
) -> AppResult<JurisdictionAssignments> {
    let mut statement = connection.prepare(
        "SELECT j.jurisdiction_key, a.party, a.role, a.responsibility
         FROM todo_assessment_jurisdictions AS j
         JOIN todo_assessment_jurisdiction_assignments AS a
           ON a.jurisdiction_id = j.id
         WHERE j.assessment_id = ?1
         ORDER BY j.jurisdiction_key, a.party, a.role, a.responsibility",
    )?;
    let rows = statement
        .query_map([assessment_id.storage_id()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut result = BTreeMap::new();
    for (key, party, role, responsibility) in rows {
        result
            .entry(key)
            .or_insert_with(Vec::new)
            .push((party, role, responsibility));
    }
    Ok(result)
}

fn canonical_assignments(assignments: &[JurisdictionAssignment]) -> Vec<AssignmentTuple> {
    let mut values = assignments
        .iter()
        .map(|assignment| {
            (
                assignment.party.clone(),
                jurisdiction_role(assignment.role).to_owned(),
                assignment.responsibility.clone(),
            )
        })
        .collect::<Vec<_>>();
    values.sort();
    values
}

fn validate_assembled_design(
    connection: &Connection,
    design_id: DesignId,
    assessment_id: SituationAssessmentId,
) -> AppResult<()> {
    let expected = assessment_jurisdiction_assignments(connection, assessment_id)?;
    let mut statement = connection.prepare(
        "SELECT id, jurisdiction_key, action
         FROM todo_design_jurisdiction_changes
         WHERE design_id = ?1 AND status = 'active' ORDER BY id",
    )?;
    let rows = statement
        .query_map([design_id.storage_id()], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = BTreeSet::new();
    let mut active = BTreeSet::new();
    for (row_id, key, action) in rows {
        if !seen.insert(key.clone()) {
            return Err(AppError::invalid(
                "duplicate_jurisdiction_change",
                format!("active design maps jurisdiction more than once: {key}"),
            ));
        }
        if action == "add" {
            if expected.contains_key(&key) {
                return Err(AppError::invalid(
                    "jurisdiction_already_exists",
                    format!("active design adds assessed jurisdiction again: {key}"),
                ));
            }
        } else {
            let assessment_assignments = expected.get(&key).ok_or_else(|| {
                AppError::invalid(
                    "unknown_jurisdiction_reference",
                    format!("active design references unknown assessed jurisdiction: {key}"),
                )
            })?;
            let mut supplied = connection
                .prepare(
                    "SELECT party, role, responsibility
                     FROM todo_design_responsibilities
                     WHERE jurisdiction_change_id = ?1 AND side = 'expected'
                     ORDER BY party, role, responsibility",
                )?
                .query_map([row_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            supplied.sort();
            if &supplied != assessment_assignments {
                return Err(AppError::conflict(
                    "jurisdiction_basis_changed",
                    format!(
                        "active expected assignments do not match assessment jurisdiction {key}"
                    ),
                ));
            }
        }
        if action != "retire" {
            active.insert(key);
        }
    }
    let missing = expected
        .keys()
        .filter(|key| !seen.contains(*key))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(AppError::invalid(
            "incomplete_jurisdiction_mapping",
            format!(
                "active design no longer maps assessed jurisdictions: {}",
                missing.join(", ")
            ),
        ));
    }
    let mut clauses = connection.prepare(
        "SELECT jurisdiction_key FROM todo_design_clauses
         WHERE design_id = ?1 AND status = 'active' AND jurisdiction_key IS NOT NULL",
    )?;
    let references = clauses
        .query_map([design_id.storage_id()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(reference) = references
        .into_iter()
        .find(|reference| !active.contains(reference))
    {
        return Err(AppError::invalid(
            "unknown_jurisdiction_reference",
            format!("active clause references retired or missing jurisdiction: {reference}"),
        ));
    }
    Ok(())
}

fn validate_design_predecessor(
    connection: &Connection,
    assessment: &SituationAssessmentView,
    based_on_design_id: Option<DesignId>,
) -> AppResult<()> {
    match based_on_design_id {
        None if assessment.based_on_design_id.is_none() => Ok(()),
        None => Err(AppError::conflict(
            "design_predecessor_missing",
            "the assessment includes an accepted design basis that must be carried forward",
        )),
        Some(id) => {
            let predecessor = get_design_tx(connection, id)?;
            if predecessor.todo_id != assessment.todo_id {
                return Err(AppError::invalid(
                    "design_predecessor_wrong_todo",
                    "the predecessor design belongs to a different todo",
                ));
            }
            if Some(id) == assessment.based_on_design_id
                || predecessor.assessment_id == Some(assessment.id)
            {
                Ok(())
            } else {
                Err(AppError::conflict(
                    "design_predecessor_wrong_lineage",
                    "the predecessor design is not in this assessment's exact lineage",
                ))
            }
        }
    }
}

fn insert_design_jurisdiction(
    transaction: &Transaction<'_>,
    design_id: DesignId,
    change: &NewJurisdictionChange,
) -> AppResult<()> {
    let slot = next_design_operation_id(transaction, design_id)?;
    transaction.execute(
        "INSERT INTO todo_design_jurisdiction_changes(
             design_id, slot, local_ref, jurisdiction_key, action, rationale
         ) VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            design_id.storage_id(),
            slot,
            change.r#ref,
            change.key,
            jurisdiction_action(change.action),
            change.rationale
        ],
    )?;
    let row_id = transaction.last_insert_rowid();
    insert_assignments(
        transaction,
        "todo_design_responsibilities",
        "jurisdiction_change_id",
        row_id,
        Some("expected"),
        &change.expected_assignments,
    )?;
    insert_assignments(
        transaction,
        "todo_design_responsibilities",
        "jurisdiction_change_id",
        row_id,
        Some("proposed"),
        &change.proposed_assignments,
    )?;
    insert_strings(
        transaction,
        "todo_design_jurisdiction_bases",
        "basis_ref",
        row_id,
        &change.basis_refs,
    )
}

fn replace_design_jurisdiction(
    transaction: &Transaction<'_>,
    design_id: DesignId,
    replacement: &crate::tool_server::contracts::JurisdictionChangeReplacement,
) -> AppResult<()> {
    let row_id = require_design_slot(
        transaction,
        "todo_design_jurisdiction_changes",
        design_id,
        &replacement.operation_id,
    )?;
    transaction.execute(
        "DELETE FROM todo_design_responsibilities
         WHERE jurisdiction_change_id = ?1",
        [row_id],
    )?;
    transaction.execute(
        "DELETE FROM todo_design_jurisdiction_bases
         WHERE jurisdiction_change_id = ?1",
        [row_id],
    )?;
    transaction.execute(
        "UPDATE todo_design_jurisdiction_changes
         SET jurisdiction_key = ?2, action = ?3, rationale = ?4, status = 'active'
         WHERE id = ?1",
        params![
            row_id,
            replacement.key,
            jurisdiction_action(replacement.action),
            replacement.rationale
        ],
    )?;
    insert_assignments(
        transaction,
        "todo_design_responsibilities",
        "jurisdiction_change_id",
        row_id,
        Some("expected"),
        &replacement.expected_assignments,
    )?;
    insert_assignments(
        transaction,
        "todo_design_responsibilities",
        "jurisdiction_change_id",
        row_id,
        Some("proposed"),
        &replacement.proposed_assignments,
    )?;
    insert_strings(
        transaction,
        "todo_design_jurisdiction_bases",
        "basis_ref",
        row_id,
        &replacement.basis_refs,
    )
}

fn insert_design_clause(
    transaction: &Transaction<'_>,
    design_id: DesignId,
    clause: &NewDesignClause,
) -> AppResult<()> {
    let slot = next_design_operation_id(transaction, design_id)?;
    transaction.execute(
        "INSERT INTO todo_design_clauses(
             design_id, slot, local_ref, kind, subject, statement, jurisdiction_key
         ) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            design_id.storage_id(),
            slot,
            clause.r#ref,
            design_clause_kind(clause.kind),
            clause.subject,
            clause.statement,
            clause.jurisdiction_ref
        ],
    )?;
    let row_id = transaction.last_insert_rowid();
    insert_strings(
        transaction,
        "todo_design_clause_bases",
        "basis_ref",
        row_id,
        &clause.basis_refs,
    )
}

fn replace_design_clause(
    transaction: &Transaction<'_>,
    design_id: DesignId,
    replacement: &crate::tool_server::contracts::DesignReplacement,
) -> AppResult<()> {
    let row_id = require_design_slot(
        transaction,
        "todo_design_clauses",
        design_id,
        &replacement.operation_id,
    )?;
    transaction.execute(
        "DELETE FROM todo_design_clause_bases WHERE clause_id = ?1",
        [row_id],
    )?;
    transaction.execute(
        "UPDATE todo_design_clauses
         SET kind = ?2, subject = ?3, statement = ?4,
             jurisdiction_key = ?5, status = 'active'
         WHERE id = ?1",
        params![
            row_id,
            design_clause_kind(replacement.kind),
            replacement.subject,
            replacement.statement,
            replacement.jurisdiction_ref
        ],
    )?;
    insert_strings(
        transaction,
        "todo_design_clause_bases",
        "basis_ref",
        row_id,
        &replacement.basis_refs,
    )
}

fn insert_design_choice(
    transaction: &Transaction<'_>,
    design_id: DesignId,
    choice: &crate::tool_server::contracts::DesignChoice,
) -> AppResult<()> {
    let slot = next_design_operation_id(transaction, design_id)?;
    transaction.execute(
        "INSERT INTO todo_design_choices(
             design_id, slot, local_ref, question, materiality
         ) VALUES(?1,?2,?3,?4,?5)",
        params![
            design_id.storage_id(),
            slot,
            choice.r#ref,
            choice.question,
            choice.why_material
        ],
    )?;
    let row_id = transaction.last_insert_rowid();
    insert_strings(
        transaction,
        "todo_design_choice_bases",
        "basis_ref",
        row_id,
        &choice.basis_refs,
    )
}

fn next_design_operation_id(connection: &Connection, design_id: DesignId) -> AppResult<String> {
    let next: i64 = connection.query_row(
        "SELECT
             (SELECT count(*) FROM todo_design_jurisdiction_changes WHERE design_id = ?1)
           + (SELECT count(*) FROM todo_design_clauses WHERE design_id = ?1)
           + (SELECT count(*) FROM todo_design_choices WHERE design_id = ?1)
           + 1",
        [design_id.storage_id()],
        |row| row.get(0),
    )?;
    Ok(format!("op-{next}"))
}

fn seal_design_if_ready(transaction: &Transaction<'_>, id: DesignId) -> AppResult<()> {
    let active_choices: i64 = transaction.query_row(
        "SELECT count(*) FROM todo_design_choices
         WHERE design_id = ?1 AND status = 'active'",
        [id.storage_id()],
        |row| row.get(0),
    )?;
    if active_choices == 0 {
        validate_design_ready_coverage(transaction, id)?;
        let version: i64 = transaction.query_row(
            "SELECT draft_version FROM todo_designs WHERE id = ?1",
            [id.storage_id()],
            |row| row.get(0),
        )?;
        transaction.execute(
            "UPDATE todo_designs
             SET state = 'ready', canonical_digest = ?2
             WHERE id = ?1",
            params![
                id.storage_id(),
                format!("design-seal-v1:{}:{version}", id.storage_id())
            ],
        )?;
    }
    Ok(())
}

fn validate_design_ready_coverage(connection: &Connection, id: DesignId) -> AppResult<()> {
    let admitted_bases = design_basis_catalog_for_design(connection, id)?;
    validate_active_design_basis_refs(connection, id, &admitted_bases)?;
    let mut statement = connection.prepare(
        "SELECT DISTINCT kind FROM todo_design_clauses
         WHERE design_id = ?1 AND status = 'active'",
    )?;
    let present_kinds = statement
        .query_map([id.storage_id()], |row| row.get::<_, String>(0))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    let missing_kinds = REQUIRED_DESIGN_CLAUSE_KINDS
        .iter()
        .filter(|kind| !present_kinds.contains(**kind))
        .copied()
        .collect::<Vec<_>>();
    if !missing_kinds.is_empty() {
        return Err(AppError::invalid(
            "incomplete_design_clause_coverage",
            format!(
                "ready design is missing clause kinds: {}",
                missing_kinds.join(", ")
            ),
        ));
    }

    let supplied_bases = active_design_basis_refs(connection, id)?;

    let mut required_direction_bases = BTreeSet::from(["direction:body".to_owned()]);
    let mut statement = connection.prepare(
        "SELECT boundary.local_ref
         FROM todo_designs AS design
         JOIN todo_situation_assessments AS assessment
           ON assessment.id = design.assessment_id
         JOIN todo_direction_boundaries AS boundary
           ON boundary.direction_revision_id = assessment.direction_revision_id
         WHERE design.id = ?1
         ORDER BY boundary.id",
    )?;
    let boundary_refs = statement
        .query_map([id.storage_id()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    required_direction_bases.extend(
        boundary_refs
            .into_iter()
            .map(|local_ref| format!("direction:{local_ref}")),
    );
    let missing_direction_bases = required_direction_bases
        .difference(&supplied_bases)
        .cloned()
        .collect::<Vec<_>>();
    if !missing_direction_bases.is_empty() {
        return Err(AppError::invalid(
            "incomplete_direction_basis_coverage",
            format!(
                "ready design is missing direction basis refs: {}",
                missing_direction_bases.join(", ")
            ),
        ));
    }

    let predecessor_id = connection.query_row(
        "SELECT based_on_design_id FROM todo_designs WHERE id = ?1",
        [id.storage_id()],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    let Some(predecessor_id) = predecessor_id else {
        return Ok(());
    };
    let predecessor = design_id(predecessor_id)?;
    let mut statement = connection.prepare(
        "SELECT slot FROM todo_design_jurisdiction_changes
         WHERE design_id = ?1 AND status = 'active'
         UNION
         SELECT slot FROM todo_design_clauses
         WHERE design_id = ?1 AND status = 'active'
         UNION
         SELECT slot FROM todo_design_choices
         WHERE design_id = ?1 AND status = 'active'",
    )?;
    let predecessor_operations = statement
        .query_map([predecessor_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let missing_predecessor_bases = predecessor_operations
        .into_iter()
        .map(|operation| format!("design:{predecessor}:{operation}"))
        .filter(|basis| !supplied_bases.contains(basis))
        .collect::<Vec<_>>();
    if !missing_predecessor_bases.is_empty() {
        return Err(AppError::invalid(
            "incomplete_predecessor_basis_coverage",
            format!(
                "ready design is missing predecessor basis refs: {}",
                missing_predecessor_bases.join(", ")
            ),
        ));
    }
    Ok(())
}

fn require_design_slot(
    connection: &Connection,
    table: &str,
    design_id: DesignId,
    slot: &str,
) -> AppResult<i64> {
    let sql = format!(
        "SELECT id FROM {table}
         WHERE design_id = ?1 AND slot = ?2 AND status = 'active'"
    );
    connection
        .query_row(&sql, params![design_id.storage_id(), slot], |row| {
            row.get(0)
        })
        .optional()?
        .ok_or_else(|| {
            AppError::invalid(
                "design_operation_not_found",
                format!("design operation not found: {slot}"),
            )
        })
}

fn drop_design_slot(
    transaction: &Transaction<'_>,
    table: &str,
    design_id: DesignId,
    drop: &crate::tool_server::contracts::DesignDrop,
) -> AppResult<()> {
    let row_id = require_design_slot(transaction, table, design_id, &drop.operation_id)?;
    let sql = format!(
        "UPDATE {table} SET status = 'dropped'
         WHERE id = ?1 AND status = 'active'"
    );
    if transaction.execute(&sql, [row_id])? != 1 {
        return Err(AppError::conflict(
            "design_operation_already_dropped",
            format!("design operation is already dropped: {}", drop.operation_id),
        ));
    }
    record_design_drop(
        transaction,
        design_id,
        &drop.operation_id,
        &drop.reason,
        &drop.basis_refs,
    )?;
    Ok(())
}

fn record_design_drop(
    transaction: &Transaction<'_>,
    design_id: DesignId,
    operation_id: &str,
    reason: &str,
    basis_refs: &[String],
) -> AppResult<()> {
    transaction.execute(
        "INSERT INTO todo_design_operation_drops(design_id, operation_id, reason)
         VALUES(?1,?2,?3)",
        params![design_id.storage_id(), operation_id, reason],
    )?;
    for (ordinal, basis_ref) in basis_refs.iter().enumerate() {
        transaction.execute(
            "INSERT INTO todo_design_operation_drop_bases(
                 design_id, operation_id, ordinal, basis_ref
             ) VALUES(?1,?2,?3,?4)",
            params![
                design_id.storage_id(),
                operation_id,
                ordinal_i64(ordinal)?,
                basis_ref,
            ],
        )?;
    }
    Ok(())
}

fn insert_routing_boundaries(
    transaction: &Transaction<'_>,
    routing_id: RoutingProposalId,
    boundaries: &[crate::tool_server::contracts::ProposedDirectionBoundary],
) -> AppResult<()> {
    for boundary in boundaries {
        transaction.execute(
            "INSERT INTO concern_routing_boundaries(
                 routing_id, local_ref, kind, statement, attribution
             ) VALUES(?1,?2,?3,?4,?5)",
            params![
                routing_id.storage_id(),
                boundary.r#ref,
                direction_boundary_kind(boundary.kind),
                boundary.text,
                direction_boundary_attribution(boundary.attribution)
            ],
        )?;
        let boundary_id = transaction.last_insert_rowid();
        for (ordinal, source_ref) in boundary.basis_refs.iter().enumerate() {
            transaction.execute(
                "INSERT INTO concern_routing_boundary_sources(
                     boundary_id, ordinal, source_ref
                 ) VALUES(?1,?2,?3)",
                params![boundary_id, ordinal_i64(ordinal)?, source_ref],
            )?;
        }
    }
    Ok(())
}

fn insert_assignments(
    transaction: &Transaction<'_>,
    table: &str,
    parent_column: &str,
    parent_id: i64,
    side: Option<&str>,
    assignments: &[JurisdictionAssignment],
) -> AppResult<()> {
    for assignment in assignments {
        if let Some(side) = side {
            let sql = format!(
                "INSERT INTO {table}(
                     {parent_column}, side, party, role, responsibility
                 ) VALUES(?1,?2,?3,?4,?5)"
            );
            transaction.execute(
                &sql,
                params![
                    parent_id,
                    side,
                    assignment.party,
                    jurisdiction_role(assignment.role),
                    assignment.responsibility,
                ],
            )?;
        } else {
            let sql = format!(
                "INSERT INTO {table}(
                     {parent_column}, party, role, responsibility
                 ) VALUES(?1,?2,?3,?4)"
            );
            transaction.execute(
                &sql,
                params![
                    parent_id,
                    assignment.party,
                    jurisdiction_role(assignment.role),
                    assignment.responsibility,
                ],
            )?;
        }
    }
    Ok(())
}

fn insert_strings(
    transaction: &Transaction<'_>,
    table: &str,
    value_column: &str,
    parent_id: i64,
    values: &[String],
) -> AppResult<()> {
    let parent_column = match table {
        "concern_routing_evidence" | "concern_routing_limitations" => "routing_id",
        "todo_assessment_identity_refs" => "assessment_id",
        "todo_assessment_finding_evidence" => "finding_id",
        "todo_assessment_jurisdiction_evidence" => "jurisdiction_id",
        "todo_assessment_unresolved_evidence" => "unresolved_id",
        "todo_design_jurisdiction_bases" => "jurisdiction_change_id",
        "todo_design_clause_bases" => "clause_id",
        "todo_design_choice_bases" => "choice_id",
        _ => return Err(stored_data_error("unsupported string child table")),
    };
    let sql = format!(
        "INSERT INTO {table}({parent_column}, ordinal, {value_column})
         VALUES(?1,?2,?3)"
    );
    for (ordinal, value) in values.iter().enumerate() {
        transaction.execute(&sql, params![parent_id, ordinal_i64(ordinal)?, value])?;
    }
    Ok(())
}

fn load_strings(
    connection: &Connection,
    table: &str,
    value_column: &str,
    parent_id: i64,
) -> AppResult<Vec<String>> {
    let parent_column = if table.starts_with("concern_routing_") {
        "routing_id"
    } else {
        return Err(stored_data_error("unsupported string read table"));
    };
    load_child_strings(connection, table, parent_column, value_column, parent_id)
}

fn load_child_strings(
    connection: &Connection,
    table: &str,
    parent_column: &str,
    value_column: &str,
    parent_id: i64,
) -> AppResult<Vec<String>> {
    let sql = format!(
        "SELECT {value_column} FROM {table}
         WHERE {parent_column} = ?1 ORDER BY ordinal"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([parent_id], |row| row.get(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn require_agent_job(
    connection: &Connection,
    id: i64,
    stage: &str,
    concern_id: Option<ConcernId>,
    todo_id: Option<TodoId>,
    base_digest: &str,
) -> AppResult<()> {
    let matches: bool = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM todo_agent_jobs
             WHERE id = ?1 AND stage = ?2
               AND concern_id IS ?3 AND todo_id IS ?4 AND base_digest = ?5
         )",
        params![
            id,
            stage,
            concern_id.map(ConcernId::storage_id),
            todo_id.map(TodoId::storage_id),
            base_digest,
        ],
        |row| row.get(0),
    )?;
    if matches {
        Ok(())
    } else {
        Err(AppError::conflict(
            "agent_job_basis_mismatch",
            "agent job does not match this Todo stage snapshot",
        ))
    }
}

fn require_exact_direction_revision(
    connection: &Connection,
    todo_id: TodoId,
    revision: i64,
) -> AppResult<()> {
    let current = current_direction(connection, todo_id)?;
    if current.revision == revision {
        Ok(())
    } else {
        Err(AppError::conflict(
            "routing_basis_changed",
            format!("direction revision changed for {todo_id}"),
        ))
    }
}

fn require_canonical_open_todo(connection: &Connection, todo_id: TodoId) -> AppResult<()> {
    let row = connection
        .query_row(
            "SELECT t.status,
                    EXISTS(SELECT 1 FROM todo_supersessions AS s
                           WHERE s.superseded_todo_id = t.id)
             FROM todos AS t WHERE t.id = ?1",
            [todo_id.storage_id()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found("todo_not_found", format!("todo not found: {todo_id}"))
        })?;
    if row.1 {
        return Err(AppError::conflict(
            "todo_superseded",
            format!("todo has been superseded: {todo_id}"),
        ));
    }
    if row.0 != "open" {
        return Err(AppError::conflict(
            "todo_not_open",
            format!("todo is not open: {todo_id}"),
        ));
    }
    Ok(())
}

fn validate_decision_source(source: &DecisionSource) -> AppResult<()> {
    validate_nonblank("decision source", &source.source_path)?;
    if !Path::new(&source.source_path).is_absolute() {
        return Err(AppError::invalid(
            "invalid_decision_source",
            "decision source path must be absolute",
        ));
    }
    Ok(())
}

fn absolute_utf8_path<'a>(path: &'a Path, name: &str) -> AppResult<&'a str> {
    if !path.is_absolute() {
        return Err(AppError::invalid(
            "invalid_source_path",
            format!("{name} path must be absolute"),
        ));
    }
    path.to_str().ok_or_else(|| {
        AppError::invalid(
            "invalid_source_path",
            format!("{name} path must contain valid UTF-8"),
        )
    })
}

fn validate_nonblank(name: &str, value: &str) -> AppResult<()> {
    if value.trim().is_empty() {
        Err(AppError::invalid(
            "blank_text",
            format!("{name} must not be blank"),
        ))
    } else {
        Ok(())
    }
}

fn now(connection: &Connection) -> AppResult<String> {
    connection
        .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
            row.get(0)
        })
        .map_err(Into::into)
}

fn parse_todo_id(value: &str) -> AppResult<TodoId> {
    value.parse().map_err(|error| {
        AppError::invalid(
            "invalid_todo_id",
            format!("invalid todo ID {value:?}: {error}"),
        )
    })
}

fn concern_id(value: i64) -> AppResult<ConcernId> {
    ConcernId::from_storage(value).map_err(|error| stored_data_error(&error.to_string()))
}
fn routing_id(value: i64) -> AppResult<RoutingProposalId> {
    RoutingProposalId::from_storage(value).map_err(|error| stored_data_error(&error.to_string()))
}
fn assessment_id(value: i64) -> AppResult<SituationAssessmentId> {
    SituationAssessmentId::from_storage(value)
        .map_err(|error| stored_data_error(&error.to_string()))
}
fn design_id(value: i64) -> AppResult<DesignId> {
    DesignId::from_storage(value).map_err(|error| stored_data_error(&error.to_string()))
}

fn ordinal_i64(value: usize) -> AppResult<i64> {
    i64::try_from(value).map_err(|_| invalid_number("ordinal"))
}

fn invalid_number(name: &str) -> AppError {
    AppError::invalid("number_out_of_range", format!("{name} is out of range"))
}

fn stored_data_error(message: &str) -> AppError {
    AppError::database("invalid_stored_data", message)
}

fn invalid_stored_text(message: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message,
        )),
    )
}

fn id_conversion(
    index: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, Type::Integer, Box::new(error))
}

fn routing_disposition(value: RoutingDisposition) -> &'static str {
    match value {
        RoutingDisposition::Attach => "attach",
        RoutingDisposition::Create => "create",
        RoutingDisposition::Revise => "revise",
        RoutingDisposition::Unify => "unify",
        RoutingDisposition::Dismiss => "dismiss",
        RoutingDisposition::Defer => "defer",
    }
}

fn direction_boundary_kind(value: DirectionBoundaryKind) -> &'static str {
    match value {
        DirectionBoundaryKind::Required => "required",
        DirectionBoundaryKind::Forbidden => "forbidden",
        DirectionBoundaryKind::Authority => "authority",
        DirectionBoundaryKind::NonGoal => "non_goal",
        DirectionBoundaryKind::Unresolved => "unresolved",
    }
}

fn direction_boundary_attribution(value: DirectionBoundaryAttribution) -> &'static str {
    match value {
        DirectionBoundaryAttribution::ExplicitUser => "explicit_user",
        DirectionBoundaryAttribution::GoverningInstruction => "governing_instruction",
        DirectionBoundaryAttribution::AcceptedInference => "accepted_inference",
    }
}

fn assessment_disposition(value: AssessmentDisposition) -> &'static str {
    match value {
        AssessmentDisposition::Ready => "ready",
        AssessmentDisposition::NeedsUserChoice => "needs_user_choice",
        AssessmentDisposition::Inconclusive => "inconclusive",
    }
}

fn finding_kind(value: FindingKind) -> &'static str {
    match value {
        FindingKind::CurrentState => "current_state",
        FindingKind::Constraint => "constraint",
        FindingKind::Dependency => "dependency",
        FindingKind::Gap => "gap",
    }
}

fn jurisdiction_role(value: JurisdictionRole) -> &'static str {
    match value {
        JurisdictionRole::Owner => "owner",
        JurisdictionRole::Participant => "participant",
        JurisdictionRole::Consumer => "consumer",
    }
}

fn boundary_disposition(value: BoundaryDisposition) -> &'static str {
    match value {
        BoundaryDisposition::Satisfied => "satisfied",
        BoundaryDisposition::Unsatisfied => "unsatisfied",
        BoundaryDisposition::ConstrainsDesign => "constrains_design",
        BoundaryDisposition::Unknown => "unknown",
    }
}

fn unresolved_kind(value: UnresolvedKind) -> &'static str {
    match value {
        UnresolvedKind::UserChoice => "user_choice",
        UnresolvedKind::EvidenceGap => "evidence_gap",
        UnresolvedKind::JurisdictionConflict => "jurisdiction_conflict",
    }
}

fn jurisdiction_action(value: JurisdictionAction) -> &'static str {
    match value {
        JurisdictionAction::Keep => "keep",
        JurisdictionAction::Move => "move",
        JurisdictionAction::Add => "add",
        JurisdictionAction::Retire => "retire",
    }
}

fn design_clause_kind(value: DesignClauseKind) -> &'static str {
    match value {
        DesignClauseKind::Ownership => "ownership",
        DesignClauseKind::Boundary => "boundary",
        DesignClauseKind::State => "state",
        DesignClauseKind::Interface => "interface",
        DesignClauseKind::Lifecycle => "lifecycle",
        DesignClauseKind::Failure => "failure",
        DesignClauseKind::Compatibility => "compatibility",
        DesignClauseKind::Acceptance => "acceptance",
        DesignClauseKind::NonGoal => "non_goal",
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use rusqlite::{Connection, params};

    use super::{
        AssessmentBase, DecisionSource, SituationAssessmentView, authorize_design,
        authorize_routing, capture_concern, create_design, create_routing_proposal,
        create_situation_assessment, get_assessment_return_for_job, get_concern, get_design,
        latest_current_ready_assessment, record_agent_job, record_assessment_return,
        record_design_correction, revise_design, routing_snapshot,
    };
    use crate::db;
    use crate::model::{ConcernId, DesignId, SituationAssessmentId, TodoId, TodoStatus};
    use crate::tool_server::contracts::{
        AssessmentDisposition, AssessmentFinding, BoundaryDisposition, ConcernRoutingProposal,
        DesignChoice, DesignClauseKind, DesignRevision, DirectionBoundaryAttribution,
        DirectionBoundaryKind, DirectionMapping, FindingKind, JurisdictionAction,
        JurisdictionAssignment, JurisdictionFinding, JurisdictionRole, NewDesignClause,
        NewJurisdictionChange, ProposedDirection, ProposedDirectionBoundary, RoutingDisposition,
        RoutingTarget, SituationAssessment, SubjectIdentity, UnifyRoute, UnresolvedAssessmentItem,
        UnresolvedKind,
    };

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn required<T>(value: Option<T>, message: &str) -> Result<T, Box<dyn std::error::Error>> {
        value.ok_or_else(|| io::Error::other(message.to_owned()).into())
    }

    fn failure<T, E>(result: Result<T, E>, message: &str) -> Result<E, Box<dyn std::error::Error>> {
        match result {
            Ok(_) => Err(io::Error::other(message.to_owned()).into()),
            Err(error) => Ok(error),
        }
    }

    #[test]
    fn routing_rejects_stale_done_and_wrong_revision_targets() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut connection = db::init(&database)?;
        let target = insert_legacy_todo(&mut connection, "Target", "Keep this outcome", true)?;
        let done = insert_legacy_todo(&mut connection, "Done", "Historical outcome", true)?;
        connection.execute(
            "UPDATE todos SET status='done', completed_at='2026-08-28T00:00:00.000Z'
             WHERE id=?1",
            [done.storage_id()],
        )?;
        let source = directory.path().join("source.md");
        std::fs::write(&source, "source")?;

        let wrong = capture_concern(&mut connection, "wrong revision", &source)?;
        let snapshot = routing_snapshot(&connection, wrong.id)?;
        assert!(
            snapshot
                .candidates
                .iter()
                .all(|candidate| candidate.status == TodoStatus::Open)
        );
        assert!(
            !snapshot
                .candidates
                .iter()
                .any(|candidate| candidate.id == done)
        );
        let target_candidate = required(
            snapshot
                .candidates
                .iter()
                .find(|candidate| candidate.id == target),
            "open target missing from routing snapshot",
        )?;
        assert_eq!(target_candidate.boundaries.len(), 1);
        assert_eq!(target_candidate.boundaries[0].local_ref, "b1");
        let job = routing_job(&mut connection, &snapshot, "job-wrong")?;
        let proposal = attach_proposal(target, 2);
        let error = failure(
            create_routing_proposal(&mut connection, &snapshot, job, "call-wrong", &proposal),
            "historical direction was accepted",
        )?;
        assert_eq!(error.code(), "routing_basis_changed");

        let completed = capture_concern(&mut connection, "done target", &source)?;
        let snapshot = routing_snapshot(&connection, completed.id)?;
        let job = routing_job(&mut connection, &snapshot, "job-done")?;
        let error = failure(
            create_routing_proposal(
                &mut connection,
                &snapshot,
                job,
                "call-done",
                &attach_proposal(done, 1),
            ),
            "done target was accepted",
        )?;
        assert_eq!(error.code(), "routing_target_not_in_snapshot");

        let stale = capture_concern(&mut connection, "stale after proposal", &source)?;
        let snapshot = routing_snapshot(&connection, stale.id)?;
        let job = routing_job(&mut connection, &snapshot, "job-stale")?;
        let proposal = create_routing_proposal(
            &mut connection,
            &snapshot,
            job,
            "call-stale",
            &attach_proposal(target, 1),
        )?;
        connection.execute(
            "UPDATE todos SET status='done', completed_at='2026-08-28T00:00:01.000Z'
             WHERE id=?1",
            [target.storage_id()],
        )?;
        let decision = DecisionSource {
            source_path: source.to_string_lossy().into_owned(),
            thread_id: None,
            turn_id: None,
        };
        let error = failure(
            authorize_routing(&mut connection, proposal.id, &decision),
            "stale routing authorization succeeded",
        )?;
        assert_eq!(error.code(), "routing_basis_changed");
        assert_eq!(
            get_concern(&connection, stale.id)?.status,
            super::ConcernStatus::Pending
        );
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // One end-to-end assessment and design lifecycle.
    #[test]
    fn design_drafts_allocate_operations_resolve_choices_and_stop_at_authorization() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut connection = db::init(&database)?;
        let todo = insert_legacy_todo(
            &mut connection,
            "Reconcile ownership",
            "Keep execution outside this design",
            true,
        )?;
        let snapshot = super::assessment_snapshot(&connection, todo)?;
        let assessment_job = record_agent_job(
            &mut connection,
            "situation_assessment",
            None,
            Some(todo),
            &snapshot.base_digest,
            "assessment-requester",
            "assessment-job",
        )?;
        let owner = JurisdictionAssignment {
            party: "Todo".to_owned(),
            role: JurisdictionRole::Owner,
            responsibility: "Own concern and design authority".to_owned(),
        };
        let assessment = SituationAssessment {
            disposition: AssessmentDisposition::Ready,
            summary: "Todo owns the retained concern; Nucleus only runs bounded research."
                .to_owned(),
            subject: SubjectIdentity {
                label: "Todo reconciliation".to_owned(),
                identity_refs: vec!["source:fixture@chars:0-20".to_owned()],
            },
            findings: vec![AssessmentFinding {
                r#ref: "f-current".to_owned(),
                kind: FindingKind::CurrentState,
                claim: "Todo persists the domain records.".to_owned(),
                evidence_refs: vec!["source:fixture@chars:0-20".to_owned()],
            }],
            jurisdictions: vec![JurisdictionFinding {
                key: "todo-domain".to_owned(),
                concern: "Durable domain authority".to_owned(),
                assignments: vec![owner.clone()],
                evidence_refs: vec!["source:fixture@chars:0-20".to_owned()],
            }],
            direction_mappings: vec![DirectionMapping {
                boundary_ref: "b1".to_owned(),
                disposition: BoundaryDisposition::ConstrainsDesign,
                finding_refs: vec!["f-current".to_owned()],
                explanation: "The design must not add execution state.".to_owned(),
            }],
            unresolved: vec![],
        };
        let mut invalid_ready_assessment = assessment.clone();
        invalid_ready_assessment.unresolved = vec![UnresolvedAssessmentItem {
            r#ref: "u-evidence".to_owned(),
            kind: UnresolvedKind::EvidenceGap,
            description: "The current deployment state is not observable.".to_owned(),
            materiality: "The present responsibility boundary cannot be established.".to_owned(),
            evidence_refs: vec!["source:fixture@chars:0-20".to_owned()],
        }];
        let error = failure(
            create_situation_assessment(
                &mut connection,
                &snapshot,
                assessment_job.id,
                "invalid-ready-assessment-call",
                &invalid_ready_assessment,
                &[],
            ),
            "a ready assessment retained an unresolved evidence gap",
        )?;
        assert_eq!(error.code(), "ready_assessment_has_unresolved_items");
        let todo_base = AssessmentBase {
            source_ref: "todo-snapshot".to_owned(),
            kind: "todo".to_owned(),
            locator: todo.to_string(),
            revision: snapshot.base_digest.clone(),
            observed_at: "2026-08-28T00:00:00.000Z".to_owned(),
        };
        let error = failure(
            create_situation_assessment(
                &mut connection,
                &snapshot,
                assessment_job.id,
                "missing-source-base-call",
                &assessment,
                std::slice::from_ref(&todo_base),
            ),
            "assessment evidence resolved to no persisted source base",
        )?;
        assert_eq!(error.code(), "unknown_assessment_source_ref");
        let assessment = create_situation_assessment(
            &mut connection,
            &snapshot,
            assessment_job.id,
            "assessment-call",
            &assessment,
            &[
                AssessmentBase {
                    source_ref: "source:fixture".to_owned(),
                    kind: "document".to_owned(),
                    locator: "/tmp/fixture.md".to_owned(),
                    revision: "fixture-v1".to_owned(),
                    observed_at: "2026-08-28T00:00:00.000Z".to_owned(),
                },
                todo_base,
            ],
        )?;
        assert_eq!(assessment.direction_revision, 1);
        assert_eq!(assessment.bases[0].source_ref, "source:fixture");

        let design_snapshot = super::assessment_snapshot(&connection, todo)?;
        let design_job = record_agent_job(
            &mut connection,
            "design_reconciliation",
            None,
            Some(todo),
            &design_snapshot.base_digest,
            "design-requester",
            "design-job",
        )?;
        let submission = crate::tool_server::contracts::DesignSubmission {
            summary: "Todo owns the durable design boundary; execution remains separate."
                .to_owned(),
            jurisdiction_changes: vec![NewJurisdictionChange {
                r#ref: "jurisdiction-local".to_owned(),
                key: "todo-domain".to_owned(),
                action: JurisdictionAction::Keep,
                expected_assignments: vec![owner.clone()],
                proposed_assignments: vec![owner],
                rationale: "The domain authority does not move.".to_owned(),
                basis_refs: vec!["direction:body".to_owned(), "direction:b1".to_owned()],
            }],
            clauses: complete_clauses(&["direction:body", "direction:b1"]),
            unresolved_choices: vec![DesignChoice {
                r#ref: "choice-local".to_owned(),
                question: "Is another execution table needed?".to_owned(),
                why_material: "Execution is explicitly out of scope.".to_owned(),
                basis_refs: vec!["direction:b1".to_owned()],
            }],
        };
        let draft = create_design(
            &mut connection,
            &assessment,
            None,
            design_job.id,
            "design-call",
            &submission,
        )?;
        assert_eq!(draft.state, "open");
        assert_eq!(draft.jurisdiction_changes[0]["operation_id"], "op-1");
        assert_eq!(draft.clauses[0]["operation_id"], "op-2");
        assert_eq!(draft.unresolved_choices[0]["operation_id"], "op-11");

        let error = failure(
            revise_design(
                &mut connection,
                draft.id,
                &DesignRevision {
                    expected_version: 1,
                    summary: None,
                    jurisdiction_replacements: vec![],
                    jurisdiction_additions: vec![],
                    jurisdiction_drops: vec![],
                    replacements: vec![],
                    additions: vec![NewDesignClause {
                        r#ref: "invented-basis-clause".to_owned(),
                        kind: DesignClauseKind::State,
                        subject: "Unadmitted state".to_owned(),
                        statement: "This operation cites an invented basis.".to_owned(),
                        basis_refs: vec!["invented:basis".to_owned()],
                        jurisdiction_ref: Some("todo-domain".to_owned()),
                    }],
                    drops: vec![],
                    unresolved_choices: None,
                },
            ),
            "a design revision admitted an invented basis ref",
        )?;
        assert_eq!(error.code(), "unknown_design_basis_reference");
        assert_eq!(get_design(&connection, draft.id)?.draft_version, 1);

        let ready = revise_design(
            &mut connection,
            draft.id,
            &DesignRevision {
                expected_version: 1,
                summary: None,
                jurisdiction_replacements: vec![],
                jurisdiction_additions: vec![],
                jurisdiction_drops: vec![],
                replacements: vec![],
                additions: vec![],
                drops: vec![],
                unresolved_choices: Some(vec![]),
            },
        )?;
        assert_eq!(ready.state, "ready");
        assert_eq!(ready.draft_version, 2);
        assert_eq!(ready.unresolved_choices[0]["status"], "dropped");
        assert!(ready.unresolved_choices[0]["drop"].is_object());

        let decision = DecisionSource {
            source_path: directory
                .path()
                .join("decision.md")
                .to_string_lossy()
                .into_owned(),
            thread_id: Some("thread-1".to_owned()),
            turn_id: Some("turn-1".to_owned()),
        };
        let (authorized, changed) = authorize_design(&mut connection, ready.id, &decision)?;
        assert!(changed);
        assert_eq!(authorized.state, "authorized");
        assert_eq!(authorized.decision_thread_id.as_deref(), Some("thread-1"));
        assert!(assessment.current);
        assert!(latest_current_ready_assessment(&connection, todo).is_err());
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // Keeps the three zero-choice completeness checks together.
    #[test]
    fn zero_choice_design_requires_complete_clause_and_direction_coverage() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut connection = db::init(&database)?;
        let (todo, assessment, design_job_id) =
            insert_assessment_return_fixture(&mut connection, "coverage")?;
        connection.execute(
            "INSERT INTO todo_assessment_jurisdictions(
                 assessment_id, jurisdiction_key, concern
             ) VALUES(?1, 'todo-domain', 'Durable domain authority')",
            [assessment.id.storage_id()],
        )?;
        let jurisdiction_id = connection.last_insert_rowid();
        connection.execute(
            "INSERT INTO todo_assessment_jurisdiction_assignments(
                 jurisdiction_id, party, role, responsibility
             ) VALUES(?1, 'Todo', 'owner', 'Own Todo domain records')",
            [jurisdiction_id],
        )?;
        let assessment = super::get_assessment(&connection, assessment.id)?;
        let owner = JurisdictionAssignment {
            party: "Todo".to_owned(),
            role: JurisdictionRole::Owner,
            responsibility: "Own Todo domain records".to_owned(),
        };
        let jurisdiction = |basis_refs: Vec<String>| NewJurisdictionChange {
            r#ref: "jurisdiction-local".to_owned(),
            key: "todo-domain".to_owned(),
            action: JurisdictionAction::Keep,
            expected_assignments: vec![owner.clone()],
            proposed_assignments: vec![owner.clone()],
            rationale: "Todo retains its domain authority.".to_owned(),
            basis_refs,
        };

        let unknown_basis = crate::tool_server::contracts::DesignSubmission {
            summary: "This submission cites evidence outside its frozen catalog.".to_owned(),
            jurisdiction_changes: vec![jurisdiction(vec!["invented:basis".to_owned()])],
            clauses: complete_clauses(&["direction:body"]),
            unresolved_choices: vec![],
        };
        let error = failure(
            create_design(
                &mut connection,
                &assessment,
                None,
                design_job_id,
                "unknown-basis-call",
                &unknown_basis,
            ),
            "an initial design admitted an invented basis ref",
        )?;
        assert_eq!(error.code(), "unknown_design_basis_reference");

        let incomplete = crate::tool_server::contracts::DesignSubmission {
            summary: "This submission omits required design semantics.".to_owned(),
            jurisdiction_changes: vec![jurisdiction(vec!["direction:body".to_owned()])],
            clauses: vec![NewDesignClause {
                r#ref: "ownership-only".to_owned(),
                kind: DesignClauseKind::Ownership,
                subject: "Todo domain".to_owned(),
                statement: "Todo owns its domain records.".to_owned(),
                basis_refs: vec!["direction:body".to_owned()],
                jurisdiction_ref: Some("todo-domain".to_owned()),
            }],
            unresolved_choices: vec![],
        };
        let error = failure(
            create_design(
                &mut connection,
                &assessment,
                None,
                design_job_id,
                "incomplete-kind-call",
                &incomplete,
            ),
            "a zero-choice design omitted required clause kinds",
        )?;
        assert_eq!(error.code(), "incomplete_design_clause_coverage");

        let assessment_basis = format!("assessment:{}", assessment.id);
        let missing_direction = crate::tool_server::contracts::DesignSubmission {
            summary: "This submission omits its direction basis.".to_owned(),
            jurisdiction_changes: vec![jurisdiction(vec![assessment_basis.clone()])],
            clauses: complete_clauses(&[assessment_basis.as_str()]),
            unresolved_choices: vec![],
        };
        let error = failure(
            create_design(
                &mut connection,
                &assessment,
                None,
                design_job_id,
                "missing-direction-call",
                &missing_direction,
            ),
            "a zero-choice design omitted direction:body",
        )?;
        assert_eq!(error.code(), "incomplete_direction_basis_coverage");
        let allocated: i64 = connection.query_row(
            "SELECT count(*) FROM todo_designs WHERE todo_id = ?1",
            [todo.storage_id()],
            |row| row.get(0),
        )?;
        assert_eq!(allocated, 0);
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // Covers both terminal umbrella states in one invariant test.
    #[test]
    fn design_authorization_requires_an_open_canonical_umbrella() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut connection = db::init(&database)?;
        let decision = DecisionSource {
            source_path: directory
                .path()
                .join("decision.md")
                .to_string_lossy()
                .into_owned(),
            thread_id: None,
            turn_id: None,
        };

        let (done_todo, done_assessment, done_job) =
            insert_assessment_return_fixture(&mut connection, "done-authorization")?;
        let done_design = insert_design_state(
            &connection,
            done_todo,
            done_assessment.id,
            done_job,
            "ready",
            "done-ready-call",
        )?;
        connection.execute(
            "UPDATE todos SET status = 'done',
                 completed_at = '2026-08-28T00:00:00.000Z' WHERE id = ?1",
            [done_todo.storage_id()],
        )?;
        let error = failure(
            authorize_design(&mut connection, done_design, &decision),
            "a design on a completed umbrella was authorized",
        )?;
        assert_eq!(error.code(), "todo_not_open");

        let (absorbed, assessment, design_job) =
            insert_assessment_return_fixture(&mut connection, "superseded-authorization")?;
        let superseded_design = insert_design_state(
            &connection,
            absorbed,
            assessment.id,
            design_job,
            "ready",
            "superseded-ready-call",
        )?;
        let survivor = insert_legacy_todo(
            &mut connection,
            "Surviving umbrella",
            "Retain the canonical concern identity",
            true,
        )?;
        let concern = capture_concern(
            &mut connection,
            "These umbrellas describe one enduring concern",
            directory.path(),
        )?;
        let snapshot = routing_snapshot(&connection, concern.id)?;
        let routing_job_id = routing_job(&mut connection, &snapshot, "supersede-for-design")?;
        let left = RoutingTarget {
            todo_id: absorbed.to_string(),
            direction_revision: 1,
        };
        let right = RoutingTarget {
            todo_id: survivor.to_string(),
            direction_revision: 1,
        };
        let proposal = ConcernRoutingProposal {
            disposition: RoutingDisposition::Unify,
            targets: vec![left.clone(), right.clone()],
            proposed_direction: Some(ProposedDirection {
                title: "Canonical umbrella".to_owned(),
                body: "Retain both historical identities under one canonical concern.".to_owned(),
                boundaries: vec![ProposedDirectionBoundary {
                    r#ref: "b-unified".to_owned(),
                    kind: DirectionBoundaryKind::Required,
                    text: "Preserve both concern histories.".to_owned(),
                    attribution: DirectionBoundaryAttribution::AcceptedInference,
                    basis_refs: vec![
                        format!("candidate:{absorbed}"),
                        format!("candidate:{survivor}"),
                    ],
                }],
            }),
            unify: Some(UnifyRoute {
                left,
                right,
                survivor_todo_id: survivor.to_string(),
            }),
            rationale: "The test fixture establishes one enduring concern.".to_owned(),
            evidence_refs: vec![
                format!("candidate:{absorbed}"),
                format!("candidate:{survivor}"),
            ],
            limitations: vec![],
        };
        let routing = create_routing_proposal(
            &mut connection,
            &snapshot,
            routing_job_id,
            "superseding-routing-call",
            &proposal,
        )?;
        authorize_routing(&mut connection, routing.id, &decision)?;
        let error = failure(
            authorize_design(&mut connection, superseded_design, &decision),
            "a design on a superseded umbrella was authorized",
        )?;
        assert_eq!(error.code(), "todo_superseded");
        Ok(())
    }

    #[test]
    fn newer_assessment_supersedes_an_older_observation() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut connection = db::init(&database)?;
        let (todo, older, _) = insert_assessment_return_fixture(&mut connection, "newer-a")?;
        let snapshot = super::assessment_snapshot(&connection, todo)?;
        let job = record_agent_job(
            &mut connection,
            "situation_assessment",
            None,
            Some(todo),
            &snapshot.base_digest,
            "newer-assessment-requester",
            "newer-assessment-job",
        )?;
        connection.execute(
            "INSERT INTO todo_situation_assessments(
                 todo_id, agent_job_id, direction_revision_id, concern_set_digest,
                 notes_through_id, based_on_design_id, disposition, summary,
                 subject_label, observed_at, producer_tool_call_id
             ) VALUES(?1,?2,?3,?4,NULL,NULL,'ready',?5,?6,?7,?8)",
            params![
                todo.storage_id(),
                job.id,
                snapshot.direction.id,
                snapshot.concern_set_digest,
                "A newer observation of the same Todo basis.",
                "Newer observed subject",
                "2026-08-28T00:00:01.000Z",
                "newer-assessment-call",
            ],
        )?;
        let newer = SituationAssessmentId::from_storage(connection.last_insert_rowid())?;
        let older = super::get_assessment(&connection, older.id)?;
        assert!(!older.current);
        assert!(
            older
                .stale_reasons
                .iter()
                .any(|reason| reason == "newer situation assessment exists")
        );
        assert_eq!(
            latest_current_ready_assessment(&connection, todo)?.id,
            newer
        );
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // One correction lifecycle with all predecessor invariants.
    #[test]
    fn ready_design_covers_every_active_predecessor_operation() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut connection = db::init(&database)?;
        let (todo, assessment, predecessor_job) =
            insert_assessment_return_fixture(&mut connection, "predecessor-coverage")?;
        let predecessor = insert_design_state(
            &connection,
            todo,
            assessment.id,
            predecessor_job,
            "open",
            "predecessor-open-call",
        )?;
        connection.execute(
            "INSERT INTO todo_design_jurisdiction_changes(
                 design_id, slot, local_ref, jurisdiction_key, action, rationale
             ) VALUES(?1, 'op-1', 'prior-jurisdiction', 'prior-domain', 'keep',
                      'Preserve prior ownership')",
            [predecessor.storage_id()],
        )?;
        connection.execute(
            "INSERT INTO todo_design_clauses(
                 design_id, slot, local_ref, kind, subject, statement
             ) VALUES(?1, 'op-2', 'prior-clause', 'ownership', 'Prior domain',
                      'Preserve prior domain authority')",
            [predecessor.storage_id()],
        )?;
        connection.execute(
            "UPDATE todo_designs
             SET state = 'ready', canonical_digest = 'predecessor-fixture-digest'
             WHERE id = ?1",
            [predecessor.storage_id()],
        )?;

        let snapshot = super::assessment_snapshot(&connection, todo)?;
        let child_job = record_agent_job(
            &mut connection,
            "design_reconciliation",
            None,
            Some(todo),
            &snapshot.base_digest,
            "child-design-requester",
            "child-design-job",
        )?;
        let correction = record_design_correction(
            &mut connection,
            child_job.id,
            predecessor,
            "Correct the predecessor without losing its active operations.",
        )?;
        assert_eq!(correction.basis_ref, format!("correction:{}", child_job.id));
        assert_eq!(
            record_design_correction(
                &mut connection,
                child_job.id,
                predecessor,
                "Correct the predecessor without losing its active operations.",
            )?,
            correction
        );
        let conflict = failure(
            record_design_correction(
                &mut connection,
                child_job.id,
                predecessor,
                "Different feedback must not replace the correction basis.",
            ),
            "correction provenance was replaced",
        )?;
        assert_eq!(conflict.code(), "design_correction_already_recorded");
        connection.execute(
            "INSERT INTO todo_designs(
                 todo_id, revision, assessment_id, based_on_design_id, agent_job_id,
                 state, summary, producer_tool_call_id
             ) VALUES(?1,2,?2,?3,?4,'open','Child design','child-design-call')",
            params![
                todo.storage_id(),
                assessment.id.storage_id(),
                predecessor.storage_id(),
                child_job.id,
            ],
        )?;
        let child = DesignId::from_storage(connection.last_insert_rowid())?;
        for (index, kind) in super::REQUIRED_DESIGN_CLAUSE_KINDS.iter().enumerate() {
            connection.execute(
                "INSERT INTO todo_design_clauses(
                     design_id, slot, local_ref, kind, subject, statement
                 ) VALUES(?1,?2,?3,?4,'Child domain','Complete desired-state clause')",
                params![
                    child.storage_id(),
                    format!("op-{}", index + 1),
                    format!("child-clause-{}", index + 1),
                    kind,
                ],
            )?;
            let clause_id = connection.last_insert_rowid();
            connection.execute(
                "INSERT INTO todo_design_clause_bases(clause_id, ordinal, basis_ref)
                 VALUES(?1,0,'direction:body')",
                [clause_id],
            )?;
        }

        let first_clause: i64 = connection.query_row(
            "SELECT id FROM todo_design_clauses
             WHERE design_id = ?1 ORDER BY id LIMIT 1",
            [child.storage_id()],
            |row| row.get(0),
        )?;
        connection.execute(
            "INSERT INTO todo_design_clause_bases(clause_id, ordinal, basis_ref)
             VALUES(?1,1,'invented:basis')",
            [first_clause],
        )?;
        let transaction = connection.transaction()?;
        let error = failure(
            super::seal_design_if_ready(&transaction, child),
            "a design cited a basis outside its admitted catalog",
        )?;
        assert_eq!(error.code(), "unknown_design_basis_reference");
        drop(transaction);
        connection.execute(
            "DELETE FROM todo_design_clause_bases
             WHERE clause_id = ?1 AND basis_ref = 'invented:basis'",
            [first_clause],
        )?;

        let transaction = connection.transaction()?;
        let error = failure(
            super::seal_design_if_ready(&transaction, child),
            "a design omitted active predecessor operations",
        )?;
        assert_eq!(error.code(), "incomplete_predecessor_basis_coverage");
        drop(transaction);

        for (ordinal, basis_ref) in [
            format!("design:{predecessor}:op-1"),
            format!("design:{predecessor}:op-2"),
            correction.basis_ref.clone(),
        ]
        .into_iter()
        .enumerate()
        {
            connection.execute(
                "INSERT INTO todo_design_clause_bases(clause_id, ordinal, basis_ref)
                 VALUES(?1,?2,?3)",
                params![first_clause, i64::try_from(ordinal + 1)?, basis_ref,],
            )?;
        }
        let transaction = connection.transaction()?;
        super::seal_design_if_ready(&transaction, child)?;
        transaction.commit()?;
        let child = get_design(&connection, child)?;
        assert_eq!(child.state, "ready");
        assert_eq!(
            child.correction_basis_ref.as_deref(),
            Some(correction.basis_ref.as_str())
        );
        assert_eq!(
            child.correction_feedback.as_deref(),
            Some("Correct the predecessor without losing its active operations.")
        );
        let immutable = failure(
            connection.execute(
                "UPDATE todo_design_corrections SET feedback = 'replacement' WHERE agent_job_id = ?1",
                [child_job.id],
            ),
            "correction provenance was mutable",
        )?;
        assert!(
            immutable
                .to_string()
                .contains("design corrections are immutable")
        );
        Ok(())
    }

    #[test]
    fn assessment_return_is_durable_before_a_draft_and_exactly_replayable() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut connection = db::init(&database)?;
        let (todo, assessment, design_job_id) =
            insert_assessment_return_fixture(&mut connection, "before-draft")?;
        let references = vec!["source:missing".to_owned(), "finding:stale".to_owned()];

        let recorded = record_assessment_return(
            &mut connection,
            design_job_id,
            assessment.id,
            "return-call",
            "The current evidence cannot settle ownership.",
            &references,
        )?;
        assert_eq!(recorded.assessment_id, assessment.id);
        assert_eq!(recorded.design_id, None);
        assert_eq!(recorded.missing_or_stale_refs, references);
        assert_eq!(
            get_assessment_return_for_job(&connection, design_job_id)?,
            Some(recorded.clone())
        );
        let Err(unavailable) = latest_current_ready_assessment(&connection, todo) else {
            return Err(io::Error::other(
                "a returned assessment was reused for another design run",
            )
            .into());
        };
        assert_eq!(unavailable.code(), "current_ready_assessment_missing");
        assert_eq!(
            record_assessment_return(
                &mut connection,
                design_job_id,
                assessment.id,
                "return-call",
                "The current evidence cannot settle ownership.",
                &references,
            )?,
            recorded
        );

        let conflict = record_assessment_return(
            &mut connection,
            design_job_id,
            assessment.id,
            "different-call",
            "The current evidence cannot settle ownership.",
            &references,
        );
        let Err(conflict) = conflict else {
            return Err(
                io::Error::other("a second return replaced the durable terminal outcome").into(),
            );
        };
        assert_eq!(conflict.code(), "assessment_return_already_recorded");

        let insert = connection.execute(
            "INSERT INTO todo_designs(
                 todo_id, revision, assessment_id, agent_job_id,
                 state, summary, producer_tool_call_id
             ) VALUES(?1,1,?2,?3,'open','late draft','late-call')",
            params![
                assessment.todo_id.storage_id(),
                assessment.id.storage_id(),
                design_job_id,
            ],
        );
        let Err(insert_error) = insert else {
            return Err(
                io::Error::other("schema allowed a draft after the terminal return").into(),
            );
        };
        assert!(
            insert_error
                .to_string()
                .contains("design job already returned for assessment")
        );
        Ok(())
    }

    #[test]
    fn assessment_return_abandons_only_an_open_draft() -> TestResult {
        let directory = tempfile::tempdir()?;
        let database = directory.path().join("todo.db");
        let mut connection = db::init(&database)?;
        let (todo, assessment, design_job_id) =
            insert_assessment_return_fixture(&mut connection, "open-draft")?;
        let open = insert_design_state(
            &connection,
            todo,
            assessment.id,
            design_job_id,
            "open",
            "open-call",
        )?;

        let returned = record_assessment_return(
            &mut connection,
            design_job_id,
            assessment.id,
            "return-open-call",
            "A material source is stale.",
            &["source:stale".to_owned()],
        )?;
        assert_eq!(returned.design_id, Some(open));
        let abandoned = get_design(&connection, open)?;
        assert_eq!(abandoned.state, "abandoned");
        assert_eq!(
            abandoned.decision_reason.as_deref(),
            Some("A material source is stale.")
        );

        let (todo, assessment, design_job_id) =
            insert_assessment_return_fixture(&mut connection, "ready-draft")?;
        let ready = insert_design_state(
            &connection,
            todo,
            assessment.id,
            design_job_id,
            "ready",
            "ready-call",
        )?;
        let error = record_assessment_return(
            &mut connection,
            design_job_id,
            assessment.id,
            "return-ready-call",
            "This return is too late.",
            &["source:late".to_owned()],
        );
        let Err(error) = error else {
            return Err(
                io::Error::other("a ready design was replaced by an assessment return").into(),
            );
        };
        assert_eq!(error.code(), "assessment_return_too_late");
        assert_eq!(get_design(&connection, ready)?.state, "ready");
        assert!(get_assessment_return_for_job(&connection, design_job_id)?.is_none());
        Ok(())
    }

    fn insert_legacy_todo(
        connection: &mut Connection,
        title: &str,
        direction: &str,
        with_boundary: bool,
    ) -> Result<TodoId, Box<dyn std::error::Error>> {
        connection.execute(
            "INSERT INTO concerns(body, source_path, status, resolved_at)
             VALUES(?1, '/tmp/legacy-source.md', 'attached',
                    strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            [direction],
        )?;
        let concern = ConcernId::from_storage(connection.last_insert_rowid())?;
        connection.execute("INSERT INTO todos DEFAULT VALUES", [])?;
        let todo = TodoId::from_storage(connection.last_insert_rowid())?;
        connection.execute(
            "INSERT INTO todo_direction_revisions(
                 todo_id, revision, title, body, source_concern_id, provenance_kind
             ) VALUES(?1,1,?2,?3,?4,'legacy_v1')",
            params![todo.storage_id(), title, direction, concern.storage_id()],
        )?;
        let direction_id = connection.last_insert_rowid();
        if with_boundary {
            connection.execute(
                "INSERT INTO todo_direction_boundaries(
                     direction_revision_id, local_ref, kind, statement, attribution
                 ) VALUES(?1,'b1','non_goal','Do not add execution state','legacy_unknown')",
                [direction_id],
            )?;
        }
        connection.execute(
            "INSERT INTO todo_concerns(todo_id, concern_id) VALUES(?1,?2)",
            params![todo.storage_id(), concern.storage_id()],
        )?;
        Ok(todo)
    }

    fn insert_assessment_return_fixture(
        connection: &mut Connection,
        suffix: &str,
    ) -> Result<(TodoId, SituationAssessmentView, i64), Box<dyn std::error::Error>> {
        let todo = insert_legacy_todo(
            connection,
            &format!("Return fixture {suffix}"),
            "Retain exact assessment provenance",
            false,
        )?;
        let snapshot = super::assessment_snapshot(connection, todo)?;
        let assessment_job = record_agent_job(
            connection,
            "situation_assessment",
            None,
            Some(todo),
            &snapshot.base_digest,
            &format!("assessment-requester-{suffix}"),
            &format!("assessment-job-{suffix}"),
        )?;
        connection.execute(
            "INSERT INTO todo_situation_assessments(
                 todo_id, agent_job_id, direction_revision_id, concern_set_digest,
                 notes_through_id, based_on_design_id, disposition, summary,
                 subject_label, observed_at, producer_tool_call_id
             ) VALUES(?1,?2,?3,?4,NULL,NULL,'ready',?5,?6,?7,?8)",
            params![
                todo.storage_id(),
                assessment_job.id,
                snapshot.direction.id,
                snapshot.concern_set_digest,
                format!("Assessment fixture {suffix}"),
                format!("Subject {suffix}"),
                "2026-08-28T00:00:00.000Z",
                format!("assessment-call-{suffix}"),
            ],
        )?;
        let assessment_id = SituationAssessmentId::from_storage(connection.last_insert_rowid())?;
        let assessment = super::get_assessment(connection, assessment_id)?;
        let design_snapshot = super::assessment_snapshot(connection, todo)?;
        let design_job = record_agent_job(
            connection,
            "design_reconciliation",
            None,
            Some(todo),
            &design_snapshot.base_digest,
            &format!("design-requester-{suffix}"),
            &format!("design-job-{suffix}"),
        )?;
        Ok((todo, assessment, design_job.id))
    }

    fn insert_design_state(
        connection: &Connection,
        todo_id: TodoId,
        assessment_id: SituationAssessmentId,
        agent_job_id: i64,
        state: &str,
        tool_call_id: &str,
    ) -> Result<DesignId, Box<dyn std::error::Error>> {
        connection.execute(
            "INSERT INTO todo_designs(
                 todo_id, revision, assessment_id, agent_job_id, state, summary,
                 canonical_digest, producer_tool_call_id
             ) VALUES(
                 ?1, 1, ?2, ?3, ?4, 'Return lifecycle fixture',
                 CASE WHEN ?4 = 'ready' THEN 'fixture-digest' ELSE NULL END, ?5
             )",
            params![
                todo_id.storage_id(),
                assessment_id.storage_id(),
                agent_job_id,
                state,
                tool_call_id,
            ],
        )?;
        Ok(DesignId::from_storage(connection.last_insert_rowid())?)
    }

    fn complete_clauses(basis_refs: &[&str]) -> Vec<NewDesignClause> {
        [
            DesignClauseKind::Ownership,
            DesignClauseKind::Boundary,
            DesignClauseKind::State,
            DesignClauseKind::Interface,
            DesignClauseKind::Lifecycle,
            DesignClauseKind::Failure,
            DesignClauseKind::Compatibility,
            DesignClauseKind::Acceptance,
            DesignClauseKind::NonGoal,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, kind)| NewDesignClause {
            r#ref: format!("clause-{}", index + 1),
            kind,
            subject: "Todo reconciliation".to_owned(),
            statement: format!("Desired-state coverage for clause kind {}.", index + 1),
            basis_refs: basis_refs
                .iter()
                .map(|reference| (*reference).to_owned())
                .collect(),
            jurisdiction_ref: Some("todo-domain".to_owned()),
        })
        .collect()
    }

    fn routing_job(
        connection: &mut Connection,
        snapshot: &super::RoutingSnapshot,
        name: &str,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        Ok(record_agent_job(
            connection,
            "concern_routing",
            Some(snapshot.concern.id),
            None,
            &snapshot.base_digest,
            &format!("requester-{name}"),
            name,
        )?
        .id)
    }

    fn attach_proposal(todo: TodoId, revision: u64) -> ConcernRoutingProposal {
        ConcernRoutingProposal {
            disposition: RoutingDisposition::Attach,
            targets: vec![RoutingTarget {
                todo_id: todo.to_string(),
                direction_revision: revision,
            }],
            proposed_direction: None,
            unify: None,
            rationale: "This concern belongs under the unchanged umbrella.".to_owned(),
            evidence_refs: vec!["source:fixture".to_owned()],
            limitations: vec![],
        }
    }
}
