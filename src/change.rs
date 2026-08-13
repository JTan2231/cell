use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

/// One complete semantic reconciliation submitted for a host-scoped work and corpus revision.
///
/// The host supplies and records the work and base revision. The reconciliation deliberately
/// contains neither storage identifiers nor ordering indices.
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

/// A path at the reconciliation's frozen base revision, or a meaningful local reference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub(crate) enum ConceptSelector {
    Existing {
        path: Vec<String>,
    },
    New {
        #[serde(rename = "new")]
        label: String,
    },
}

/// Exact source language, with optional natural-language disambiguators.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        under: Option<ConceptSelector>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<ConceptSelector>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<ConceptSelector>,
        evidence: Vec<EvidenceSelector>,
    },
    AddEvidence {
        concept: ConceptSelector,
        evidence: Vec<EvidenceSelector>,
    },
    RemoveEvidence {
        concept: ConceptSelector,
        evidence: Vec<EvidenceSelector>,
    },
    MoveConcept {
        concept: ConceptSelector,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        under: Option<ConceptSelector>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<ConceptSelector>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<ConceptSelector>,
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

/// Parse and validate one strict JSON reconciliation.
pub(crate) fn parse_reconciliation(
    document: &str,
) -> Result<Reconciliation, ReconciliationContractError> {
    let reconciliation: Reconciliation =
        serde_json::from_str(document).map_err(ReconciliationContractError::InvalidJson)?;
    reconciliation.validate()?;
    Ok(reconciliation)
}

/// Parse and validate an already-decoded JSON reconciliation.
pub(crate) fn parse_reconciliation_value(
    value: Value,
) -> Result<Reconciliation, ReconciliationContractError> {
    let reconciliation: Reconciliation =
        serde_json::from_value(value).map_err(ReconciliationContractError::InvalidJson)?;
    reconciliation.validate()?;
    Ok(reconciliation)
}

fn validate_operations(operations: &[ChangeOperation]) -> Result<(), ReconciliationContractError> {
    if operations.is_empty() {
        return invalid("a reconciliation must contain at least one operation");
    }

    let mut created_labels = BTreeSet::new();
    for operation in operations {
        if let ChangeOperation::CreateConcept { label, .. } = operation {
            validate_label("created concept label", label)?;
            if !created_labels.insert(normalize_label(label)) {
                return invalid(format!(
                    "created concept labels must be unique after normalization: {label:?}"
                ));
            }
        }
    }

    for operation in operations {
        validate_operation(operation, &created_labels)?;
    }
    Ok(())
}

fn validate_operation(
    operation: &ChangeOperation,
    created_labels: &BTreeSet<String>,
) -> Result<(), ReconciliationContractError> {
    match operation {
        ChangeOperation::CreateConcept {
            under,
            before,
            after,
            evidence,
            ..
        } => {
            validate_placement(
                under.as_ref(),
                before.as_ref(),
                after.as_ref(),
                created_labels,
            )?;
            validate_evidence_list("create_concept", evidence)
        }
        ChangeOperation::AddEvidence { concept, evidence } => {
            validate_selector(concept, created_labels)?;
            validate_evidence_list("add_evidence", evidence)
        }
        ChangeOperation::RemoveEvidence { concept, evidence } => {
            validate_selector(concept, created_labels)?;
            validate_evidence_list("remove_evidence", evidence)
        }
        ChangeOperation::MoveConcept {
            concept,
            under,
            before,
            after,
        } => {
            validate_selector(concept, created_labels)?;
            validate_placement(
                under.as_ref(),
                before.as_ref(),
                after.as_ref(),
                created_labels,
            )
        }
        ChangeOperation::RewordConcept { concept, label, .. } => {
            validate_selector(concept, created_labels)?;
            validate_label("reworded concept label", label)
        }
        ChangeOperation::RetireConcept {
            concept,
            replacement,
        } => {
            validate_selector(concept, created_labels)?;
            if let Some(replacement) = replacement {
                validate_selector(replacement, created_labels)?;
            }
            Ok(())
        }
    }
}

fn validate_placement(
    under: Option<&ConceptSelector>,
    before: Option<&ConceptSelector>,
    after: Option<&ConceptSelector>,
    created_labels: &BTreeSet<String>,
) -> Result<(), ReconciliationContractError> {
    if before.is_some() && after.is_some() {
        return invalid("a placement may specify before or after, but not both");
    }
    for selector in [under, before, after].into_iter().flatten() {
        validate_selector(selector, created_labels)?;
    }
    Ok(())
}

fn validate_selector(
    selector: &ConceptSelector,
    created_labels: &BTreeSet<String>,
) -> Result<(), ReconciliationContractError> {
    match selector {
        ConceptSelector::Existing { path } => {
            if path.is_empty() {
                return invalid("an existing concept path cannot be empty");
            }
            for component in path {
                validate_label("concept path component", component)?;
            }
            Ok(())
        }
        ConceptSelector::New { label } => {
            validate_label("new-concept reference", label)?;
            if created_labels.contains(&normalize_label(label)) {
                Ok(())
            } else {
                invalid(format!(
                    "new-concept reference {label:?} has no matching create_concept operation"
                ))
            }
        }
    }
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

fn normalize_label(label: &str) -> String {
    let mut normalized = String::new();
    let mut pending_space = false;
    for character in label.nfkc().flat_map(char::to_lowercase) {
        if character.is_whitespace() {
            pending_space = !normalized.is_empty();
        } else {
            if pending_space {
                normalized.push(' ');
                pending_space = false;
            }
            normalized.push(character);
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::{
        ChangeOperation, ConceptSelector, EvidenceDisposition, ReconciliationContractError,
        parse_reconciliation, parse_reconciliation_value,
    };
    use serde_json::json;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn parses_a_complete_language_level_reconciliation() -> TestResult {
        let reconciliation = parse_reconciliation(
            r#"{
                "summary": "Integrate serializable execution",
                "operations": [
                    {
                        "action": "create_concept",
                        "label": "Predicate locking",
                        "under": {"path": ["Database systems", "Concurrency control"]},
                        "after": {"path": ["Database systems", "Concurrency control", "Two-phase locking"]},
                        "evidence": [{
                            "quote": "Predicate locks prevent inserts that change the predicate result.",
                            "within_heading": ["Transactions", "Avoiding phantom reads"]
                        }]
                    },
                    {
                        "action": "move_concept",
                        "concept": {"path": ["Database systems", "Phantom prevention"]},
                        "under": {"new": "Predicate locking"}
                    },
                    {
                        "action": "reword_concept",
                        "concept": {"new": "Predicate locking"},
                        "label": "Predicate-based locking",
                        "evidence_disposition": "retain"
                    }
                ],
                "annotations": [
                    "The work presents predicate locking as a phantom-prevention technique."
                ]
            }"#,
        )?;

        assert_eq!(reconciliation.operations().len(), 3);
        assert_eq!(reconciliation.annotations().len(), 1);
        assert!(matches!(
            &reconciliation.operations()[1],
            ChangeOperation::MoveConcept {
                under: Some(ConceptSelector::New { label }),
                ..
            } if label == "Predicate locking"
        ));
        assert!(matches!(
            reconciliation.operations()[2],
            ChangeOperation::RewordConcept {
                evidence_disposition: EvidenceDisposition::Retain,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn annotations_are_optional_and_inert_contract_data() -> TestResult {
        let reconciliation = parse_reconciliation_value(json!({
            "summary": "Attach supporting evidence",
            "operations": [{
                "action": "add_evidence",
                "concept": {"path": ["Serializable execution"]},
                "evidence": [{"quote": "Equivalent to a serial execution."}]
            }]
        }))?;

        assert!(reconciliation.annotations().is_empty());
        let serialized = serde_json::to_value(reconciliation)?;
        assert!(serialized.get("annotations").is_none());
        Ok(())
    }

    #[test]
    fn rejects_unknown_fields_at_every_level() -> TestResult {
        assert_invalid_json(
            r#"{"outcome":"no_change","summary":"Done","reason":"Covered","operations":[{"action":"retire_concept","concept":{"path":["Old"]}}]}"#,
        )?;
        assert_invalid_json(
            r#"{"summary":"Update","operations":[{"action":"retire_concept","concept":{"path":["Old"]},"position":2}]}"#,
        )?;
        assert_invalid_json(
            r#"{"summary":"Update","operations":[{"action":"retire_concept","concept":{"path":["Old"],"node_id":7}}]}"#,
        )?;
        assert_invalid_json(
            r#"{"summary":"Update","operations":[{"action":"add_evidence","concept":{"path":["Old"]},"evidence":[{"quote":"Exact language","start_byte":12}]}]}"#,
        )?;
        assert_invalid_json(
            r#"{"summary":"Update","operations":[{"action":"retire_concept","concept":{"path":["Old"]}}],"uncertainties":[]}"#,
        )?;
        Ok(())
    }

    #[test]
    fn rejects_empty_reconciliations_evidence_and_annotations() -> TestResult {
        assert_invalid_reconciliation(
            r#"{"summary":"Update","operations":[]}"#,
            "at least one operation",
        )?;
        assert_invalid_reconciliation(
            r#"{"summary":"Update","operations":[{"action":"add_evidence","concept":{"path":["Concurrency"]},"evidence":[]}]}"#,
            "at least one exact evidence",
        )?;
        assert_invalid_reconciliation(
            r#"{"summary":"Update","operations":[{"action":"retire_concept","concept":{"path":["Old"]}}],"annotations":["  "]}"#,
            "annotation cannot be empty",
        )?;
        Ok(())
    }

    #[test]
    fn rejects_ambiguous_placement() -> TestResult {
        assert_invalid_reconciliation(
            r#"{
                "summary":"Move a concept",
                "operations":[{
                    "action":"move_concept",
                    "concept":{"path":["Old"]},
                    "before":{"path":["Before"]},
                    "after":{"path":["After"]}
                }]
            }"#,
            "before or after",
        )?;
        Ok(())
    }

    #[test]
    fn rejects_duplicate_normalized_creation_labels() -> TestResult {
        assert_invalid_reconciliation(
            r#"{
                "summary":"Create concepts",
                "operations":[
                    {"action":"create_concept","label":"Predicate  Locking","evidence":[{"quote":"First quote"}]},
                    {"action":"create_concept","label":"ＰＲＥＤＩＣＡＴＥ locking","evidence":[{"quote":"Second quote"}]}
                ]
            }"#,
            "unique after normalization",
        )?;
        Ok(())
    }

    #[test]
    fn rejects_a_dangling_new_concept_reference() -> TestResult {
        assert_invalid_reconciliation(
            r#"{
                "summary":"Move a concept",
                "operations":[{
                    "action":"move_concept",
                    "concept":{"path":["Existing"]},
                    "under":{"new":"Missing concept"}
                }]
            }"#,
            "no matching create_concept",
        )?;
        Ok(())
    }

    #[test]
    fn accepts_a_normalized_new_concept_reference() -> TestResult {
        parse_reconciliation(
            r#"{
                "summary":"Create and populate a concept",
                "operations":[
                    {"action":"create_concept","label":"Predicate Locking","evidence":[{"quote":"A grounded claim"}]},
                    {"action":"add_evidence","concept":{"new":"predicate locking"},"evidence":[{"quote":"More evidence"}]}
                ]
            }"#,
        )?;
        Ok(())
    }

    #[test]
    fn root_placement_needs_no_integer_position() -> TestResult {
        let reconciliation = parse_reconciliation(
            r#"{
                "summary":"Promote a concept to a root",
                "operations":[{
                    "action":"move_concept",
                    "concept":{"path":["Database systems", "Transactions"]}
                }]
            }"#,
        )?;
        let serialized = serde_json::to_value(reconciliation)?;
        assert!(serialized["operations"][0].get("under").is_none());
        assert!(serialized["operations"][0].get("position").is_none());
        Ok(())
    }

    fn assert_invalid_json(document: &str) -> TestResult {
        match parse_reconciliation(document) {
            Err(ReconciliationContractError::InvalidJson(_)) => Ok(()),
            Err(error) => Err(format!("expected a JSON contract error, got {error}").into()),
            Ok(_) => Err("expected the reconciliation to be rejected".into()),
        }
    }

    fn assert_invalid_reconciliation(document: &str, expected: &str) -> TestResult {
        match parse_reconciliation(document) {
            Err(ReconciliationContractError::InvalidReconciliation(message))
                if message.contains(expected) =>
            {
                Ok(())
            }
            Err(error) => Err(format!("expected {expected:?} in error, got {error}").into()),
            Ok(_) => Err("expected the reconciliation to be rejected".into()),
        }
    }
}
