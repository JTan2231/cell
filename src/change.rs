use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

/// One complete semantic proposal submitted for a host-scoped work and corpus revision.
///
/// The host supplies and records the work and base revision. The proposal deliberately
/// contains neither storage identifiers nor ordering indices.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ChangeProposal {
    Change {
        summary: String,
        operations: Vec<ChangeOperation>,
        uncertainties: Vec<String>,
    },
    NoChange {
        summary: String,
        reason: String,
        uncertainties: Vec<String>,
    },
}

impl ChangeProposal {
    pub(crate) fn summary(&self) -> &str {
        match self {
            Self::Change { summary, .. } | Self::NoChange { summary, .. } => summary,
        }
    }

    pub(crate) fn uncertainties(&self) -> &[String] {
        match self {
            Self::Change { uncertainties, .. } | Self::NoChange { uncertainties, .. } => {
                uncertainties
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn operations(&self) -> &[ChangeOperation] {
        match self {
            Self::Change { operations, .. } => operations,
            Self::NoChange { .. } => &[],
        }
    }

    fn validate(&self) -> Result<(), ChangeContractError> {
        validate_narrative("summary", self.summary())?;
        for uncertainty in self.uncertainties() {
            validate_narrative("uncertainty", uncertainty)?;
        }

        match self {
            Self::Change { operations, .. } => validate_operations(operations),
            Self::NoChange { reason, .. } => validate_narrative("reason", reason),
        }
    }
}

/// A path at the proposal's frozen base revision, or a meaningful local reference.
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

/// One semantic operation in an atomic proposal.
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
pub(crate) enum ChangeContractError {
    #[error("change proposal is not valid contract JSON: {0}")]
    InvalidJson(#[source] serde_json::Error),
    #[error("invalid change proposal: {0}")]
    InvalidProposal(String),
}

/// Parse and validate one strict JSON change proposal.
pub(crate) fn parse_change_proposal(document: &str) -> Result<ChangeProposal, ChangeContractError> {
    let proposal: ChangeProposal =
        serde_json::from_str(document).map_err(ChangeContractError::InvalidJson)?;
    proposal.validate()?;
    Ok(proposal)
}

/// Parse and validate an already-decoded JSON change proposal.
pub(crate) fn parse_change_proposal_value(
    value: Value,
) -> Result<ChangeProposal, ChangeContractError> {
    let proposal: ChangeProposal =
        serde_json::from_value(value).map_err(ChangeContractError::InvalidJson)?;
    proposal.validate()?;
    Ok(proposal)
}

fn validate_operations(operations: &[ChangeOperation]) -> Result<(), ChangeContractError> {
    if operations.is_empty() {
        return invalid("a change outcome must contain at least one operation");
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
) -> Result<(), ChangeContractError> {
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
) -> Result<(), ChangeContractError> {
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
) -> Result<(), ChangeContractError> {
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
) -> Result<(), ChangeContractError> {
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

fn validate_label(name: &str, value: &str) -> Result<(), ChangeContractError> {
    validate_narrative(name, value)?;
    if value.trim() != value {
        return invalid(format!("{name} cannot have leading or trailing whitespace"));
    }
    Ok(())
}

fn validate_narrative(name: &str, value: &str) -> Result<(), ChangeContractError> {
    if value.trim().is_empty() {
        return invalid(format!("{name} cannot be empty"));
    }
    Ok(())
}

fn invalid<T>(message: impl Into<String>) -> Result<T, ChangeContractError> {
    Err(ChangeContractError::InvalidProposal(message.into()))
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
        ChangeContractError, ChangeOperation, ChangeProposal, ConceptSelector, EvidenceDisposition,
        parse_change_proposal, parse_change_proposal_value,
    };
    use serde_json::json;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn parses_a_complete_language_level_change() -> TestResult {
        let proposal = parse_change_proposal(
            r#"{
                "outcome": "change",
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
                "uncertainties": []
            }"#,
        )?;

        let ChangeProposal::Change { operations, .. } = proposal else {
            return Err("expected a change proposal".into());
        };
        assert_eq!(operations.len(), 3);
        assert!(matches!(
            &operations[1],
            ChangeOperation::MoveConcept {
                under: Some(ConceptSelector::New { label }),
                ..
            } if label == "Predicate locking"
        ));
        assert!(matches!(
            operations[2],
            ChangeOperation::RewordConcept {
                evidence_disposition: EvidenceDisposition::Retain,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn parses_a_no_change_result_without_operations() -> TestResult {
        let proposal = parse_change_proposal_value(json!({
            "outcome": "no_change",
            "summary": "The relevant material is already represented",
            "reason": "Every distinct claim and quotation is already present.",
            "uncertainties": []
        }))?;

        assert!(matches!(proposal, ChangeProposal::NoChange { .. }));
        assert!(proposal.operations().is_empty());
        Ok(())
    }

    #[test]
    fn rejects_unknown_fields_at_every_level() -> TestResult {
        assert_invalid_json(
            r#"{"outcome":"no_change","summary":"Done","reason":"Covered","uncertainties":[],"base_revision":17}"#,
        )?;
        assert_invalid_json(
            r#"{"outcome":"change","summary":"Update","operations":[{"action":"retire_concept","concept":{"path":["Old"]},"position":2}],"uncertainties":[]}"#,
        )?;
        assert_invalid_json(
            r#"{"outcome":"change","summary":"Update","operations":[{"action":"retire_concept","concept":{"path":["Old"],"node_id":7}}],"uncertainties":[]}"#,
        )?;
        assert_invalid_json(
            r#"{"outcome":"change","summary":"Update","operations":[{"action":"add_evidence","concept":{"path":["Old"]},"evidence":[{"quote":"Exact language","start_byte":12}]}],"uncertainties":[]}"#,
        )?;
        Ok(())
    }

    #[test]
    fn rejects_empty_changes_and_evidence() -> TestResult {
        assert_invalid_proposal(
            r#"{"outcome":"change","summary":"Update","operations":[],"uncertainties":[]}"#,
            "at least one operation",
        )?;
        assert_invalid_proposal(
            r#"{"outcome":"change","summary":"Update","operations":[{"action":"add_evidence","concept":{"path":["Concurrency"]},"evidence":[]}],"uncertainties":[]}"#,
            "at least one exact evidence",
        )?;
        Ok(())
    }

    #[test]
    fn rejects_ambiguous_placement() -> TestResult {
        assert_invalid_proposal(
            r#"{
                "outcome":"change",
                "summary":"Move a concept",
                "operations":[{
                    "action":"move_concept",
                    "concept":{"path":["Old"]},
                    "before":{"path":["Before"]},
                    "after":{"path":["After"]}
                }],
                "uncertainties":[]
            }"#,
            "before or after",
        )?;
        Ok(())
    }

    #[test]
    fn rejects_duplicate_normalized_creation_labels() -> TestResult {
        assert_invalid_proposal(
            r#"{
                "outcome":"change",
                "summary":"Create concepts",
                "operations":[
                    {"action":"create_concept","label":"Predicate  Locking","evidence":[{"quote":"First quote"}]},
                    {"action":"create_concept","label":"ＰＲＥＤＩＣＡＴＥ locking","evidence":[{"quote":"Second quote"}]}
                ],
                "uncertainties":[]
            }"#,
            "unique after normalization",
        )?;
        Ok(())
    }

    #[test]
    fn rejects_a_dangling_new_concept_reference() -> TestResult {
        assert_invalid_proposal(
            r#"{
                "outcome":"change",
                "summary":"Move a concept",
                "operations":[{
                    "action":"move_concept",
                    "concept":{"path":["Existing"]},
                    "under":{"new":"Missing concept"}
                }],
                "uncertainties":[]
            }"#,
            "no matching create_concept",
        )?;
        Ok(())
    }

    #[test]
    fn accepts_a_normalized_new_concept_reference() -> TestResult {
        parse_change_proposal(
            r#"{
                "outcome":"change",
                "summary":"Create and populate a concept",
                "operations":[
                    {"action":"create_concept","label":"Predicate Locking","evidence":[{"quote":"A grounded claim"}]},
                    {"action":"add_evidence","concept":{"new":"predicate locking"},"evidence":[{"quote":"More evidence"}]}
                ],
                "uncertainties":[]
            }"#,
        )?;
        Ok(())
    }

    #[test]
    fn root_placement_needs_no_integer_position() -> TestResult {
        let proposal = parse_change_proposal(
            r#"{
                "outcome":"change",
                "summary":"Promote a concept to a root",
                "operations":[{
                    "action":"move_concept",
                    "concept":{"path":["Database systems", "Transactions"]}
                }],
                "uncertainties":[]
            }"#,
        )?;
        let serialized = serde_json::to_value(proposal)?;
        assert!(serialized["operations"][0].get("under").is_none());
        assert!(serialized["operations"][0].get("position").is_none());
        Ok(())
    }

    fn assert_invalid_json(document: &str) -> TestResult {
        match parse_change_proposal(document) {
            Err(ChangeContractError::InvalidJson(_)) => Ok(()),
            Err(error) => Err(format!("expected a JSON contract error, got {error}").into()),
            Ok(_) => Err("expected the proposal to be rejected".into()),
        }
    }

    fn assert_invalid_proposal(document: &str, expected: &str) -> TestResult {
        match parse_change_proposal(document) {
            Err(ChangeContractError::InvalidProposal(message)) if message.contains(expected) => {
                Ok(())
            }
            Err(error) => Err(format!("expected {expected:?} in error, got {error}").into()),
            Ok(_) => Err("expected the proposal to be rejected".into()),
        }
    }
}
