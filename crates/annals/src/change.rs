use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

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
