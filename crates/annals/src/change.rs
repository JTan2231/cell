use std::collections::{BTreeSet, HashMap};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::error::AppError;
use crate::model::ConceptId;

/// One complete semantic reconciliation submitted for a host-scoped work and corpus revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reconciliation {
    pub(crate) summary: String,
    pub(crate) operations: Vec<ChangeOperation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) annotations: Vec<String>,
}

impl Reconciliation {
    pub(crate) fn summary(&self) -> &str {
        &self.summary
    }

    pub(crate) fn operations(&self) -> &[ChangeOperation] {
        &self.operations
    }

    pub(crate) fn annotations(&self) -> &[String] {
        &self.annotations
    }

    fn validate(&self) -> Result<(), ReconciliationContractError> {
        validate_narrative("summary", self.summary())?;
        validate_operations(self.operations())?;
        for annotation in self.annotations() {
            validate_narrative("annotation", annotation)?;
        }
        Ok(())
    }
}

/// A durable public concept ID, or a request-local creation handle.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub(crate) enum ConceptSelector {
    Existing {
        id: ConceptId,
    },
    New {
        #[serde(rename = "new")]
        handle: String,
    },
}

/// Exact source language, with optional natural-language disambiguators.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EvidenceSelector {
    pub quote: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within_heading: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preceded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followed_by: Option<String>,
}

/// What to do with existing evidence when wording changes without changing identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EvidenceDisposition {
    Retain,
    Remove,
}

/// One semantic operation in an atomic reconciliation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ChangeOperation {
    CreateConcept {
        #[serde(rename = "ref")]
        handle: String,
        label: String,
        parents: Vec<ConceptSelector>,
        evidence: Vec<EvidenceSelector>,
    },
    AddParent {
        concept: ConceptSelector,
        parent: ConceptSelector,
    },
    RemoveParent {
        concept: ConceptSelector,
        parent: ConceptSelector,
    },
    AddEvidence {
        concept: ConceptSelector,
        evidence: Vec<EvidenceSelector>,
    },
    RemoveEvidence {
        concept: ConceptSelector,
        evidence: Vec<EvidenceSelector>,
    },
    RewordConcept {
        concept: ConceptSelector,
        label: String,
        evidence_disposition: EvidenceDisposition,
    },
    RetireConcept {
        concept: ConceptSelector,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replacement: Option<ConceptSelector>,
    },
}

/// A syntax or language-level contract failure, before corpus resolution begins.
#[derive(Debug, Error)]
pub(crate) enum ReconciliationContractError {
    #[error("reconciliation is not valid contract JSON: {0}")]
    InvalidJson(#[source] serde_json::Error),
    #[error("invalid reconciliation: {0}")]
    InvalidReconciliation(String),
}

pub(crate) fn parse_reconciliation(
    document: &str,
) -> Result<Reconciliation, ReconciliationContractError> {
    let reconciliation: Reconciliation =
        serde_json::from_str(document).map_err(ReconciliationContractError::InvalidJson)?;
    reconciliation.validate()?;
    Ok(reconciliation)
}

#[cfg(test)]
pub(crate) fn parse_reconciliation_value(
    value: Value,
) -> Result<Reconciliation, ReconciliationContractError> {
    let reconciliation: Reconciliation =
        serde_json::from_value(value).map_err(ReconciliationContractError::InvalidJson)?;
    reconciliation.validate()?;
    Ok(reconciliation)
}

/// Parse and validate one operation without requiring the rest of its request-local namespace.
///
/// Reconciliation drafts use this boundary so one invalid operation does not prevent valid
/// siblings from being retained. Declaration membership and request-wide conflicts are checked
/// after every operation has been inspected.
pub(crate) fn parse_operation_value(
    value: Value,
) -> Result<ChangeOperation, ReconciliationContractError> {
    let operation: ChangeOperation =
        serde_json::from_value(value).map_err(ReconciliationContractError::InvalidJson)?;
    validate_operation_content(&operation)?;
    Ok(operation)
}

/// Reserve durable concept identities for every creation in a request.
pub(crate) fn reserve_create_ids(
    transaction: &Transaction<'_>,
    reconciliation: &Reconciliation,
) -> Result<HashMap<String, i64>, AppError> {
    let mut ids = HashMap::new();
    for operation in reconciliation.operations() {
        if let ChangeOperation::CreateConcept { handle, .. } = operation {
            let id = reserve_concept_identity(transaction)?;
            ids.insert(handle.clone(), id);
        }
    }
    Ok(ids)
}

pub(crate) fn reserve_concept_identity(transaction: &Transaction<'_>) -> Result<i64, AppError> {
    transaction.execute("INSERT INTO concept_identities DEFAULT VALUES", [])?;
    let id = transaction.last_insert_rowid();
    if id <= 0 {
        return Err(AppError::database(
            "identity_overflow",
            "concept identity space is exhausted",
        ));
    }
    Ok(id)
}

/// Persist one complete, contract-valid request as normalized typed rows.
pub(crate) fn insert_request(
    transaction: &Transaction<'_>,
    work_id: i64,
    base_revision: i64,
    reconciliation: &Reconciliation,
    created_ids: &HashMap<String, i64>,
    created_at: &str,
) -> Result<i64, AppError> {
    reconciliation
        .validate()
        .map_err(|error| invalid_stored_request(error.to_string()))?;
    transaction.execute(
        "INSERT INTO reconciliation_requests(work_id, base_revision, summary, created_at)
         VALUES(?1, ?2, ?3, ?4)",
        params![work_id, base_revision, reconciliation.summary(), created_at],
    )?;
    let request_id = transaction.last_insert_rowid();
    replace_annotations(transaction, request_id, reconciliation.annotations())?;
    for (ordinal, operation) in reconciliation.operations().iter().enumerate() {
        let ordinal = i64::try_from(ordinal).map_err(|_| identity_overflow())?;
        let slot = ordinal.checked_add(1).ok_or_else(identity_overflow)?;
        let created_id = match operation {
            ChangeOperation::CreateConcept { handle, .. } => {
                Some(*created_ids.get(handle).ok_or_else(|| {
                    AppError::database(
                        "concept_binding_missing",
                        format!("creation handle {handle:?} has no reserved concept identity"),
                    )
                })?)
            }
            _ => None,
        };
        insert_operation(
            transaction,
            request_id,
            slot,
            ordinal,
            Some(operation),
            created_id,
            "staged",
            None,
            1,
            1,
        )?;
    }
    Ok(request_id)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_operation(
    transaction: &Transaction<'_>,
    request_id: i64,
    slot: i64,
    ordinal: i64,
    operation: Option<&ChangeOperation>,
    created_concept_id: Option<i64>,
    status: &str,
    hint: Option<&str>,
    created_version: i64,
    changed_version: i64,
) -> Result<i64, AppError> {
    let scalars = operation.map(operation_scalars);
    let reserved = match (operation, created_concept_id) {
        (Some(ChangeOperation::CreateConcept { .. }), Some(id)) => Some(id),
        (Some(ChangeOperation::CreateConcept { .. }), None) => {
            Some(reserve_concept_identity(transaction)?)
        }
        (_, Some(_)) => {
            return Err(AppError::database(
                "invalid_request_operation",
                "a non-creation operation cannot own a created concept identity",
            ));
        }
        _ => None,
    };
    let (action, local_ref, label, disposition) = scalars.unwrap_or_default();
    transaction.execute(
        "INSERT INTO request_operations(
             request_id, slot, ordinal, action, local_ref, label,
             evidence_disposition, created_concept_id, status, hint,
             created_version, last_changed_version
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            request_id,
            slot,
            ordinal,
            action,
            local_ref,
            label,
            disposition,
            reserved,
            status,
            hint,
            created_version,
            changed_version,
        ],
    )?;
    let operation_id = transaction.last_insert_rowid();
    if let Some(operation) = operation {
        insert_operation_children(transaction, operation_id, operation)?;
    }
    Ok(operation_id)
}

pub(crate) fn replace_operation(
    transaction: &Transaction<'_>,
    request_id: i64,
    slot: i64,
    operation: Option<&ChangeOperation>,
    changed_version: i64,
) -> Result<(), AppError> {
    let (operation_id, existing_created_id) = transaction
        .query_row(
            "SELECT id, created_concept_id FROM request_operations
             WHERE request_id = ?1 AND slot = ?2 AND status <> 'dropped'",
            params![request_id, slot],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found(
                "staged_operation_not_found",
                format!("op-{slot} is not an active operation in this reconciliation draft"),
            )
        })?;
    transaction.execute(
        "DELETE FROM operation_selectors WHERE operation_id = ?1",
        [operation_id],
    )?;
    transaction.execute(
        "DELETE FROM operation_evidence_headings
         WHERE evidence_id IN (
             SELECT id FROM operation_evidence WHERE operation_id = ?1
         )",
        [operation_id],
    )?;
    transaction.execute(
        "DELETE FROM operation_evidence WHERE operation_id = ?1",
        [operation_id],
    )?;
    let scalars = operation.map(operation_scalars);
    let created_id = match operation {
        Some(ChangeOperation::CreateConcept { .. }) => {
            Some(existing_created_id.map_or_else(|| reserve_concept_identity(transaction), Ok)?)
        }
        _ => None,
    };
    let (action, local_ref, label, disposition) = scalars.unwrap_or_default();
    transaction.execute(
        "UPDATE request_operations
         SET action = ?1, local_ref = ?2, label = ?3,
             evidence_disposition = ?4, created_concept_id = ?5,
             status = 'needs_revision', hint = NULL, last_changed_version = ?6
         WHERE id = ?7",
        params![
            action,
            local_ref,
            label,
            disposition.as_deref(),
            created_id,
            changed_version,
            operation_id,
        ],
    )?;
    if let Some(operation) = operation {
        insert_operation_children(transaction, operation_id, operation)?;
    }
    Ok(())
}

pub(crate) fn replace_request_metadata(
    transaction: &Transaction<'_>,
    request_id: i64,
    summary: &str,
    annotations: &[String],
) -> Result<(), AppError> {
    validate_reconciliation_metadata(summary, annotations)
        .map_err(|error| invalid_stored_request(error.to_string()))?;
    transaction.execute(
        "UPDATE reconciliation_requests SET summary = ?1 WHERE id = ?2",
        params![summary, request_id],
    )?;
    replace_annotations(transaction, request_id, annotations)
}

pub(crate) fn load_request(
    connection: &Connection,
    request_id: i64,
) -> Result<Reconciliation, AppError> {
    let (summary,): (String,) = connection
        .query_row(
            "SELECT summary FROM reconciliation_requests WHERE id = ?1",
            [request_id],
            |row| Ok((row.get(0)?,)),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::database(
                "reconciliation_request_missing",
                format!("normalized reconciliation request {request_id} is missing"),
            )
        })?;
    let annotations = load_annotations(connection, request_id)?;
    let operations = load_operations(connection, request_id, false)?
        .into_iter()
        .map(|operation| {
            operation.operation.ok_or_else(|| {
                AppError::database(
                    "invalid_reconciliation_request",
                    format!("request {request_id} contains an untyped active operation"),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let reconciliation = Reconciliation {
        summary,
        operations,
        annotations,
    };
    reconciliation
        .validate()
        .map_err(|error| invalid_stored_request(error.to_string()))?;
    Ok(reconciliation)
}

#[derive(Debug, Clone)]
pub(crate) struct StoredOperation {
    pub slot: i64,
    pub operation: Option<ChangeOperation>,
    pub status: String,
    pub hint: Option<String>,
}

pub(crate) fn load_operations(
    connection: &Connection,
    request_id: i64,
    include_dropped: bool,
) -> Result<Vec<StoredOperation>, AppError> {
    let clause = if include_dropped {
        ""
    } else {
        "AND status <> 'dropped'"
    };
    let sql = format!(
        "SELECT id, slot, action, local_ref, label, evidence_disposition,
                status, hint
         FROM request_operations
         WHERE request_id = ?1 {clause} ORDER BY ordinal"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([request_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;
    let mut operations = Vec::new();
    for row in rows {
        let (row_id, slot, action, local_ref, label, disposition, status, hint) = row?;
        let operation = action
            .map(|action| {
                load_typed_operation(
                    connection,
                    row_id,
                    &action,
                    local_ref,
                    label,
                    disposition.as_deref(),
                )
            })
            .transpose()?;
        operations.push(StoredOperation {
            slot,
            operation,
            status,
            hint,
        });
    }
    Ok(operations)
}

pub(crate) fn created_id_bindings(
    connection: &Connection,
    request_id: i64,
) -> Result<HashMap<String, i64>, AppError> {
    let mut statement = connection.prepare(
        "SELECT local_ref, created_concept_id
         FROM request_operations
         WHERE request_id = ?1 AND status <> 'dropped' AND action = 'create_concept'
         ORDER BY ordinal",
    )?;
    let rows = statement.query_map([request_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut bindings = HashMap::new();
    for row in rows {
        let (handle, id) = row?;
        if bindings.insert(handle.clone(), id).is_some() {
            return Err(AppError::database(
                "invalid_reconciliation_request",
                format!("creation handle {handle:?} is duplicated"),
            ));
        }
    }
    Ok(bindings)
}

fn replace_annotations(
    transaction: &Transaction<'_>,
    request_id: i64,
    annotations: &[String],
) -> Result<(), AppError> {
    transaction.execute(
        "DELETE FROM request_annotations WHERE request_id = ?1",
        [request_id],
    )?;
    for (ordinal, annotation) in annotations.iter().enumerate() {
        transaction.execute(
            "INSERT INTO request_annotations(request_id, ordinal, text)
             VALUES(?1, ?2, ?3)",
            params![
                request_id,
                i64::try_from(ordinal).map_err(|_| identity_overflow())?,
                annotation,
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn load_annotations(
    connection: &Connection,
    request_id: i64,
) -> Result<Vec<String>, AppError> {
    let mut statement = connection
        .prepare("SELECT text FROM request_annotations WHERE request_id = ?1 ORDER BY ordinal")?;
    let rows = statement.query_map([request_id], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

type OperationScalars = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn operation_scalars(operation: &ChangeOperation) -> OperationScalars {
    match operation {
        ChangeOperation::CreateConcept { handle, label, .. } => (
            Some("create_concept".into()),
            Some(handle.clone()),
            Some(label.clone()),
            None,
        ),
        ChangeOperation::AddParent { .. } => (Some("add_parent".into()), None, None, None),
        ChangeOperation::RemoveParent { .. } => (Some("remove_parent".into()), None, None, None),
        ChangeOperation::AddEvidence { .. } => (Some("add_evidence".into()), None, None, None),
        ChangeOperation::RemoveEvidence { .. } => {
            (Some("remove_evidence".into()), None, None, None)
        }
        ChangeOperation::RewordConcept {
            label,
            evidence_disposition,
            ..
        } => (
            Some("reword_concept".into()),
            None,
            Some(label.clone()),
            Some(
                match evidence_disposition {
                    EvidenceDisposition::Retain => "retain",
                    EvidenceDisposition::Remove => "remove",
                }
                .into(),
            ),
        ),
        ChangeOperation::RetireConcept { .. } => (Some("retire_concept".into()), None, None, None),
    }
}

fn insert_operation_children(
    transaction: &Transaction<'_>,
    operation_id: i64,
    operation: &ChangeOperation,
) -> Result<(), AppError> {
    match operation {
        ChangeOperation::CreateConcept {
            parents, evidence, ..
        } => {
            insert_selectors(transaction, operation_id, "parent", parents)?;
            insert_evidence(transaction, operation_id, evidence)?;
        }
        ChangeOperation::AddParent { concept, parent }
        | ChangeOperation::RemoveParent { concept, parent } => {
            insert_selectors(
                transaction,
                operation_id,
                "concept",
                std::slice::from_ref(concept),
            )?;
            insert_selectors(
                transaction,
                operation_id,
                "parent",
                std::slice::from_ref(parent),
            )?;
        }
        ChangeOperation::AddEvidence { concept, evidence }
        | ChangeOperation::RemoveEvidence { concept, evidence } => {
            insert_selectors(
                transaction,
                operation_id,
                "concept",
                std::slice::from_ref(concept),
            )?;
            insert_evidence(transaction, operation_id, evidence)?;
        }
        ChangeOperation::RewordConcept { concept, .. } => insert_selectors(
            transaction,
            operation_id,
            "concept",
            std::slice::from_ref(concept),
        )?,
        ChangeOperation::RetireConcept {
            concept,
            replacement,
        } => {
            insert_selectors(
                transaction,
                operation_id,
                "concept",
                std::slice::from_ref(concept),
            )?;
            if let Some(replacement) = replacement {
                insert_selectors(
                    transaction,
                    operation_id,
                    "replacement",
                    std::slice::from_ref(replacement),
                )?;
            }
        }
    }
    Ok(())
}

fn insert_selectors(
    transaction: &Transaction<'_>,
    operation_id: i64,
    role: &str,
    selectors: &[ConceptSelector],
) -> Result<(), AppError> {
    for (ordinal, selector) in selectors.iter().enumerate() {
        let (kind, concept_id, local_ref) = match selector {
            ConceptSelector::Existing { id } => ("existing", Some(id.storage_id()), None),
            ConceptSelector::New { handle } => ("local", None, Some(handle.as_str())),
        };
        transaction.execute(
            "INSERT INTO operation_selectors(
                 operation_id, role, ordinal, selector_kind, concept_id, local_ref
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                operation_id,
                role,
                i64::try_from(ordinal).map_err(|_| identity_overflow())?,
                kind,
                concept_id,
                local_ref,
            ],
        )?;
    }
    Ok(())
}

fn insert_evidence(
    transaction: &Transaction<'_>,
    operation_id: i64,
    evidence: &[EvidenceSelector],
) -> Result<(), AppError> {
    for (ordinal, selector) in evidence.iter().enumerate() {
        transaction.execute(
            "INSERT INTO operation_evidence(
                 operation_id, ordinal, quote, preceded_by, followed_by
             ) VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                operation_id,
                i64::try_from(ordinal).map_err(|_| identity_overflow())?,
                selector.quote,
                selector.preceded_by,
                selector.followed_by,
            ],
        )?;
        let evidence_id = transaction.last_insert_rowid();
        for (heading_ordinal, component) in selector
            .within_heading
            .as_deref()
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            transaction.execute(
                "INSERT INTO operation_evidence_headings(evidence_id, ordinal, component)
                 VALUES(?1, ?2, ?3)",
                params![
                    evidence_id,
                    i64::try_from(heading_ordinal).map_err(|_| identity_overflow())?,
                    component,
                ],
            )?;
        }
    }
    Ok(())
}

fn load_typed_operation(
    connection: &Connection,
    operation_id: i64,
    action: &str,
    local_ref: Option<String>,
    label: Option<String>,
    disposition: Option<&str>,
) -> Result<ChangeOperation, AppError> {
    let concepts = load_selectors(connection, operation_id, "concept")?;
    let parents = load_selectors(connection, operation_id, "parent")?;
    let replacements = load_selectors(connection, operation_id, "replacement")?;
    let evidence = load_evidence(connection, operation_id)?;
    let one = |mut values: Vec<ConceptSelector>, role: &str| {
        if values.len() == 1 {
            Ok(values.remove(0))
        } else {
            Err(AppError::database(
                "invalid_request_operation",
                format!("operation {operation_id} must have exactly one {role} selector"),
            ))
        }
    };
    match action {
        "create_concept" => Ok(ChangeOperation::CreateConcept {
            handle: local_ref.ok_or_else(|| invalid_operation(operation_id))?,
            label: label.ok_or_else(|| invalid_operation(operation_id))?,
            parents,
            evidence,
        }),
        "add_parent" => Ok(ChangeOperation::AddParent {
            concept: one(concepts, "concept")?,
            parent: one(parents, "parent")?,
        }),
        "remove_parent" => Ok(ChangeOperation::RemoveParent {
            concept: one(concepts, "concept")?,
            parent: one(parents, "parent")?,
        }),
        "add_evidence" => Ok(ChangeOperation::AddEvidence {
            concept: one(concepts, "concept")?,
            evidence,
        }),
        "remove_evidence" => Ok(ChangeOperation::RemoveEvidence {
            concept: one(concepts, "concept")?,
            evidence,
        }),
        "reword_concept" => Ok(ChangeOperation::RewordConcept {
            concept: one(concepts, "concept")?,
            label: label.ok_or_else(|| invalid_operation(operation_id))?,
            evidence_disposition: match disposition {
                Some("retain") => EvidenceDisposition::Retain,
                Some("remove") => EvidenceDisposition::Remove,
                _ => return Err(invalid_operation(operation_id)),
            },
        }),
        "retire_concept" => Ok(ChangeOperation::RetireConcept {
            concept: one(concepts, "concept")?,
            replacement: match replacements.len() {
                0 => None,
                1 => Some(one(replacements, "replacement")?),
                _ => return Err(invalid_operation(operation_id)),
            },
        }),
        _ => Err(invalid_operation(operation_id)),
    }
}

fn load_selectors(
    connection: &Connection,
    operation_id: i64,
    role: &str,
) -> Result<Vec<ConceptSelector>, AppError> {
    let mut statement = connection.prepare(
        "SELECT selector_kind, concept_id, local_ref
         FROM operation_selectors
         WHERE operation_id = ?1 AND role = ?2 ORDER BY ordinal",
    )?;
    let rows = statement.query_map(params![operation_id, role], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (kind, concept_id, local_ref) = row?;
        match (kind.as_str(), concept_id, local_ref) {
            ("existing", Some(id), None) => Ok(ConceptSelector::Existing {
                id: ConceptId::from_storage(id)
                    .map_err(|error| AppError::database("invalid_concept_id", error.to_string()))?,
            }),
            ("local", None, Some(handle)) => Ok(ConceptSelector::New { handle }),
            _ => Err(invalid_operation(operation_id)),
        }
    })
    .collect()
}

fn load_evidence(
    connection: &Connection,
    operation_id: i64,
) -> Result<Vec<EvidenceSelector>, AppError> {
    let mut statement = connection.prepare(
        "SELECT id, quote, preceded_by, followed_by
         FROM operation_evidence WHERE operation_id = ?1 ORDER BY ordinal",
    )?;
    let rows = statement.query_map([operation_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut output = Vec::new();
    for row in rows {
        let (evidence_id, quote, preceded_by, followed_by) = row?;
        let mut headings = connection.prepare(
            "SELECT component FROM operation_evidence_headings
             WHERE evidence_id = ?1 ORDER BY ordinal",
        )?;
        let heading_rows = headings.query_map([evidence_id], |row| row.get::<_, String>(0))?;
        let heading = heading_rows.collect::<Result<Vec<_>, _>>()?;
        output.push(EvidenceSelector {
            quote,
            within_heading: (!heading.is_empty()).then_some(heading),
            preceded_by,
            followed_by,
        });
    }
    Ok(output)
}

fn invalid_operation(operation_id: i64) -> AppError {
    AppError::database(
        "invalid_request_operation",
        format!("operation row {operation_id} has inconsistent typed fields"),
    )
}

fn invalid_stored_request(message: String) -> AppError {
    AppError::database("invalid_reconciliation_request", message)
}

fn identity_overflow() -> AppError {
    AppError::database("identity_overflow", "request identity space is exhausted")
}

pub(crate) fn validate_reconciliation_metadata(
    summary: &str,
    annotations: &[String],
) -> Result<(), ReconciliationContractError> {
    validate_narrative("summary", summary)?;
    for annotation in annotations {
        validate_narrative("annotation", annotation)?;
    }
    Ok(())
}

fn validate_operations(operations: &[ChangeOperation]) -> Result<(), ReconciliationContractError> {
    if operations.is_empty() {
        return invalid("a reconciliation must contain at least one operation");
    }

    let mut handles = BTreeSet::new();
    for operation in operations {
        if let ChangeOperation::CreateConcept { handle, label, .. } = operation {
            validate_handle(handle)?;
            validate_label("created concept label", label)?;
            if !handles.insert(handle.clone()) {
                return invalid(format!(
                    "create_concept ref values must be request-unique: {handle:?}"
                ));
            }
        }
    }

    for operation in operations {
        validate_operation(operation, &handles)?;
    }
    Ok(())
}

fn validate_operation(
    operation: &ChangeOperation,
    handles: &BTreeSet<String>,
) -> Result<(), ReconciliationContractError> {
    validate_operation_content(operation)?;
    for selector in operation_selectors(operation) {
        validate_selector_declaration(selector, handles)?;
    }
    Ok(())
}

fn validate_operation_content(
    operation: &ChangeOperation,
) -> Result<(), ReconciliationContractError> {
    match operation {
        ChangeOperation::CreateConcept {
            handle,
            label,
            parents,
            evidence,
        } => {
            validate_handle(handle)?;
            validate_label("created concept label", label)?;
            for parent in parents {
                validate_selector_content(parent)?;
            }
            validate_evidence_list("create_concept", evidence)
        }
        ChangeOperation::AddParent { concept, parent }
        | ChangeOperation::RemoveParent { concept, parent } => {
            validate_selector_content(concept)?;
            validate_selector_content(parent)
        }
        ChangeOperation::AddEvidence { concept, evidence } => {
            validate_selector_content(concept)?;
            validate_evidence_list("add_evidence", evidence)
        }
        ChangeOperation::RemoveEvidence { concept, evidence } => {
            validate_selector_content(concept)?;
            validate_evidence_list("remove_evidence", evidence)
        }
        ChangeOperation::RewordConcept { concept, label, .. } => {
            validate_selector_content(concept)?;
            validate_label("reworded concept label", label)
        }
        ChangeOperation::RetireConcept {
            concept,
            replacement,
        } => {
            validate_selector_content(concept)?;
            if let Some(replacement) = replacement {
                validate_selector_content(replacement)?;
            }
            Ok(())
        }
    }
}

fn operation_selectors(operation: &ChangeOperation) -> Vec<&ConceptSelector> {
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

fn validate_selector_content(
    selector: &ConceptSelector,
) -> Result<(), ReconciliationContractError> {
    match selector {
        ConceptSelector::Existing { .. } => Ok(()),
        ConceptSelector::New { handle } => validate_handle(handle),
    }
}

fn validate_selector_declaration(
    selector: &ConceptSelector,
    handles: &BTreeSet<String>,
) -> Result<(), ReconciliationContractError> {
    match selector {
        ConceptSelector::Existing { .. } => Ok(()),
        ConceptSelector::New { handle } => {
            if handles.contains(handle) {
                Ok(())
            } else {
                invalid(format!(
                    "new-concept reference {handle:?} has no matching create_concept ref"
                ))
            }
        }
    }
}

fn validate_handle(value: &str) -> Result<(), ReconciliationContractError> {
    validate_label("new-concept ref", value)
}

fn validate_evidence_list(
    action: &str,
    evidence: &[EvidenceSelector],
) -> Result<(), ReconciliationContractError> {
    if evidence.is_empty() {
        return invalid(format!(
            "{action} must include at least one exact evidence selector"
        ));
    }
    for selector in evidence {
        validate_narrative("evidence quote", &selector.quote)?;
        if let Some(path) = &selector.within_heading {
            if path.is_empty() {
                return invalid("within_heading cannot be an empty path");
            }
            for component in path {
                validate_label("within_heading component", component)?;
            }
        }
        for (name, value) in [
            ("preceded_by", selector.preceded_by.as_deref()),
            ("followed_by", selector.followed_by.as_deref()),
        ] {
            if let Some(value) = value {
                validate_narrative(name, value)?;
            }
        }
    }
    Ok(())
}

fn validate_label(name: &str, value: &str) -> Result<(), ReconciliationContractError> {
    validate_narrative(name, value)?;
    if value.trim() != value {
        return invalid(format!("{name} cannot have leading or trailing whitespace"));
    }
    if value.chars().any(char::is_control) {
        return invalid(format!("{name} cannot contain control characters"));
    }
    Ok(())
}

fn validate_narrative(name: &str, value: &str) -> Result<(), ReconciliationContractError> {
    if value.trim().is_empty() {
        return invalid(format!("{name} cannot be empty"));
    }
    Ok(())
}

fn invalid<T>(message: impl Into<String>) -> Result<T, ReconciliationContractError> {
    Err(ReconciliationContractError::InvalidReconciliation(
        message.into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        ChangeOperation, ConceptSelector, ReconciliationContractError, parse_reconciliation,
    };

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn parses_graph_native_reconciliation() -> TestResult {
        let reconciliation = parse_reconciliation(
            r#"{
                "summary": "Integrate transaction retries",
                "operations": [
                    {
                        "action": "create_concept",
                        "ref": "retries",
                        "label": "Transaction retries",
                        "parents": [{"id":"c7"}, {"id":"c18"}],
                        "evidence": [{"quote":"Retries can repeat a transaction."}]
                    },
                    {
                        "action": "add_parent",
                        "concept": {"new":"retries"},
                        "parent": {"id":"c21"}
                    }
                ]
            }"#,
        )?;

        assert_eq!(reconciliation.operations().len(), 2);
        assert!(matches!(
            &reconciliation.operations()[1],
            ChangeOperation::AddParent {
                concept: ConceptSelector::New { handle },
                ..
            } if handle == "retries"
        ));
        Ok(())
    }

    #[test]
    fn labels_may_repeat_but_creation_refs_may_not() -> TestResult {
        parse_reconciliation(
            r#"{
                "summary":"Two meanings",
                "operations":[
                    {"action":"create_concept","ref":"one","label":"Locks","parents":[],"evidence":[{"quote":"One."}]},
                    {"action":"create_concept","ref":"two","label":"Locks","parents":[],"evidence":[{"quote":"Two."}]}
                ]
            }"#,
        )?;
        assert_invalid(
            r#"{
                "summary":"Bad refs",
                "operations":[
                    {"action":"create_concept","ref":"same","label":"One","parents":[],"evidence":[{"quote":"One."}]},
                    {"action":"create_concept","ref":"same","label":"Two","parents":[],"evidence":[{"quote":"Two."}]}
                ]
            }"#,
            "request-unique",
        )
    }

    #[test]
    fn rejects_paths_order_and_moves() {
        for document in [
            r#"{"summary":"Old","operations":[{"action":"add_evidence","concept":{"path":["Old"]},"evidence":[{"quote":"Exact."}]}]}"#,
            r#"{"summary":"Old","operations":[{"action":"move_concept","concept":{"id":"c1"}}]}"#,
            r#"{"summary":"Old","operations":[{"action":"create_concept","ref":"x","label":"X","before":{"id":"c1"},"evidence":[{"quote":"Exact."}]}]}"#,
        ] {
            assert!(matches!(
                parse_reconciliation(document),
                Err(ReconciliationContractError::InvalidJson(_))
            ));
        }
    }

    #[test]
    fn validates_public_ids_and_local_references() -> TestResult {
        assert!(matches!(
            parse_reconciliation(
                r#"{"summary":"Bad","operations":[{"action":"retire_concept","concept":{"id":"42"}}]}"#
            ),
            Err(ReconciliationContractError::InvalidJson(_))
        ));
        assert_invalid(
            r#"{"summary":"Bad","operations":[{"action":"retire_concept","concept":{"new":"missing"}}]}"#,
            "no matching",
        )
    }

    fn assert_invalid(document: &str, expected: &str) -> TestResult {
        match parse_reconciliation(document) {
            Err(ReconciliationContractError::InvalidReconciliation(message))
                if message.contains(expected) =>
            {
                Ok(())
            }
            Err(error) => Err(format!("expected {expected:?} in error, got {error}").into()),
            Ok(_) => Err("expected reconciliation rejection".into()),
        }
    }
}
