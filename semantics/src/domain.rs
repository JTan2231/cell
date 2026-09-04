use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const PARTICIPATION_MARKER_PREFIX: &str = "Semantics-Project: ";
const MAX_TEXT: usize = 16_384;
const MAX_EFFECTS: usize = 256;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Active,
    Paused,
    Retired,
}

impl ProjectStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Retired => "retired",
        }
    }
}

impl fmt::Display for ProjectStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ProjectStatus {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "active" => Ok(Self::Active),
            "paused" => Ok(Self::Paused),
            "retired" => Ok(Self::Retired),
            _ => Err(Error::domain(
                "project_status_invalid",
                format!("unknown project status {value:?}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub status: ProjectStatus,
    pub current_path: String,
    pub activation_cursor: String,
    pub scan_cursor: String,
    pub annals_library_id: Option<String>,
    pub annals_activation_cursor: Option<String>,
    pub annals_scan_cursor: Option<String>,
    pub next_concept_number: u64,
    pub current_revision: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PathHistory {
    pub path: String,
    pub activation_cursor: String,
    pub annals_activation_cursor: Option<String>,
    pub opened_at: String,
    pub closed_at: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectDetail {
    #[serde(flatten)]
    pub project: Project,
    pub paths: Vec<PathHistory>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SemanticEffect {
    Define {
        concept_id: String,
        label: String,
        meaning: String,
    },
    Revise {
        concept_id: String,
        label: Option<String>,
        meaning: Option<String>,
    },
    Differentiate {
        concept_id: String,
        other_concept_id: String,
        distinction: String,
    },
    Reopen {
        concept_id: String,
        reason: String,
    },
    Retire {
        concept_id: String,
        reason: String,
        replacement_concept_id: Option<String>,
    },
    Ground {
        concept_id: String,
        source: GroundingSource,
        statement: String,
    },
    Unground {
        concept_id: String,
        event_id: String,
        decision_id: String,
        withdrawal_event_id: String,
        reason: String,
    },
}

impl SemanticEffect {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Define { .. } => "define",
            Self::Revise { .. } => "revise",
            Self::Differentiate { .. } => "differentiate",
            Self::Reopen { .. } => "reopen",
            Self::Retire { .. } => "retire",
            Self::Ground { .. } => "ground",
            Self::Unground { .. } => "unground",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GroundingSource {
    Decision {
        event_id: String,
        decision_id: String,
    },
    AnnalsDecisionAccount {
        library_id: String,
        event_id: String,
        account_id: String,
    },
    Seed {
        source_label: String,
        digest: String,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Grounding {
    pub revision: u64,
    pub source: GroundingSource,
    pub statement: String,
    pub active: bool,
    pub withdrawn_revision: Option<u64>,
    pub withdrawal_event_id: Option<String>,
    pub withdrawal_reason: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Distinction {
    pub revision: u64,
    pub other_concept_id: String,
    pub statement: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Concept {
    pub id: String,
    pub label: String,
    pub meaning: String,
    pub active: bool,
    pub replacement_concept_id: Option<String>,
    pub created_revision: u64,
    pub changed_revision: u64,
    pub grounds: Vec<Grounding>,
    pub distinctions: Vec<Distinction>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Repository {
    pub project_id: String,
    pub revision: u64,
    pub concepts: BTreeMap<String, Concept>,
}

impl Repository {
    #[must_use]
    pub fn empty(project_id: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            revision: 0,
            concepts: BTreeMap::new(),
        }
    }

    pub fn apply_revision(&mut self, revision: u64, effects: &[SemanticEffect]) -> Result<()> {
        if revision != self.revision + 1 {
            return Err(Error::domain(
                "revision_noncontiguous",
                format!(
                    "revision {revision} does not follow repository revision {}",
                    self.revision
                ),
            ));
        }
        let mut candidate = self.clone();
        candidate.apply_revision_in_place(revision, effects)?;
        *self = candidate;
        Ok(())
    }

    fn apply_revision_in_place(&mut self, revision: u64, effects: &[SemanticEffect]) -> Result<()> {
        validate_effect_count(effects)?;
        let mut defined = BTreeSet::new();
        for effect in effects {
            self.apply_effect(revision, effect, &mut defined)?;
        }
        self.validate_active_labels()?;
        self.revision = revision;
        Ok(())
    }

    fn apply_effect(
        &mut self,
        revision: u64,
        effect: &SemanticEffect,
        defined: &mut BTreeSet<String>,
    ) -> Result<()> {
        match effect {
            SemanticEffect::Define {
                concept_id,
                label,
                meaning,
            } => {
                validate_concept_id(concept_id)?;
                validate_text("label", label)?;
                validate_text("meaning", meaning)?;
                if self.concepts.contains_key(concept_id) || !defined.insert(concept_id.clone()) {
                    return Err(Error::domain(
                        "concept_already_exists",
                        format!("concept {concept_id} already exists"),
                    ));
                }
                self.concepts.insert(
                    concept_id.clone(),
                    Concept {
                        id: concept_id.clone(),
                        label: label.trim().to_owned(),
                        meaning: meaning.trim().to_owned(),
                        active: true,
                        replacement_concept_id: None,
                        created_revision: revision,
                        changed_revision: revision,
                        grounds: Vec::new(),
                        distinctions: Vec::new(),
                    },
                );
            }
            SemanticEffect::Revise {
                concept_id,
                label,
                meaning,
            } => {
                if label.is_none() && meaning.is_none() {
                    return Err(Error::domain(
                        "revision_empty",
                        "revise requires a label or meaning",
                    ));
                }
                if let Some(label) = label {
                    validate_text("label", label)?;
                }
                if let Some(meaning) = meaning {
                    validate_text("meaning", meaning)?;
                }
                let concept = self.active_concept_mut(concept_id)?;
                if let Some(label) = label {
                    concept.label = label.trim().to_owned();
                }
                if let Some(meaning) = meaning {
                    concept.meaning = meaning.trim().to_owned();
                }
                concept.changed_revision = revision;
            }
            SemanticEffect::Differentiate {
                concept_id,
                other_concept_id,
                distinction,
            } => {
                validate_text("distinction", distinction)?;
                if concept_id == other_concept_id {
                    return Err(Error::domain(
                        "distinction_self_reference",
                        "differentiate requires two different concepts",
                    ));
                }
                self.active_concept(other_concept_id)?;
                let concept = self.active_concept_mut(concept_id)?;
                concept.distinctions.push(Distinction {
                    revision,
                    other_concept_id: other_concept_id.clone(),
                    statement: distinction.trim().to_owned(),
                });
                concept.changed_revision = revision;
            }
            SemanticEffect::Reopen { concept_id, reason } => {
                validate_text("reason", reason)?;
                let concept = self.concept_mut(concept_id)?;
                if concept.active {
                    return Err(Error::domain(
                        "concept_not_retired",
                        format!("concept {concept_id} is already active"),
                    ));
                }
                concept.active = true;
                concept.replacement_concept_id = None;
                concept.changed_revision = revision;
            }
            SemanticEffect::Retire {
                concept_id,
                reason,
                replacement_concept_id,
            } => {
                validate_text("reason", reason)?;
                if replacement_concept_id.as_deref() == Some(concept_id) {
                    return Err(Error::domain(
                        "replacement_self_reference",
                        "a concept cannot replace itself",
                    ));
                }
                if let Some(replacement) = replacement_concept_id {
                    self.active_concept(replacement)?;
                }
                let concept = self.active_concept_mut(concept_id)?;
                concept.active = false;
                concept.replacement_concept_id = replacement_concept_id.clone();
                concept.changed_revision = revision;
            }
            SemanticEffect::Ground {
                concept_id,
                source,
                statement,
            } => {
                validate_grounding_source(source)?;
                validate_text("statement", statement)?;
                if matches!(
                    source,
                    GroundingSource::Decision { .. }
                        | GroundingSource::AnnalsDecisionAccount { .. }
                ) && self.concepts.values().any(|concept| {
                    concept
                        .grounds
                        .iter()
                        .any(|grounding| grounding.active && grounding.source == *source)
                }) {
                    return Err(Error::domain(
                        "grounding_already_active",
                        "the source already grounds an active concept",
                    ));
                }
                let concept = self.active_concept_mut(concept_id)?;
                concept.grounds.push(Grounding {
                    revision,
                    source: source.clone(),
                    statement: statement.trim().to_owned(),
                    active: true,
                    withdrawn_revision: None,
                    withdrawal_event_id: None,
                    withdrawal_reason: None,
                });
                concept.changed_revision = revision;
            }
            SemanticEffect::Unground {
                concept_id,
                event_id,
                decision_id,
                withdrawal_event_id,
                reason,
            } => {
                validate_text("event_id", event_id)?;
                validate_text("decision_id", decision_id)?;
                validate_text("withdrawal_event_id", withdrawal_event_id)?;
                validate_text("reason", reason)?;
                let concept = self.concept_mut(concept_id)?;
                let grounding = concept
                    .grounds
                    .iter_mut()
                    .find(|grounding| {
                        grounding.active
                            && matches!(
                                &grounding.source,
                                GroundingSource::Decision {
                                    event_id: prior_event,
                                    decision_id: prior_decision,
                                } if prior_event == event_id && prior_decision == decision_id
                            )
                    })
                    .ok_or_else(|| {
                        Error::domain(
                            "grounding_not_found",
                            format!(
                                "concept {concept_id} has no active grounding for event {event_id} and decision {decision_id}"
                            ),
                        )
                    })?;
                grounding.active = false;
                grounding.withdrawn_revision = Some(revision);
                grounding.withdrawal_event_id = Some(withdrawal_event_id.clone());
                grounding.withdrawal_reason = Some(reason.trim().to_owned());
                concept.changed_revision = revision;
            }
        }
        Ok(())
    }

    fn validate_active_labels(&self) -> Result<()> {
        let mut labels = BTreeMap::new();
        for concept in self.concepts.values().filter(|concept| concept.active) {
            let normalized = normalize_label(&concept.label);
            if let Some(existing) = labels.insert(normalized.clone(), concept.id.as_str()) {
                return Err(Error::domain(
                    "active_label_duplicate",
                    format!(
                        "active concepts {existing} and {} share normalized label {normalized:?}",
                        concept.id
                    ),
                ));
            }
        }
        Ok(())
    }

    fn active_concept(&self, concept_id: &str) -> Result<&Concept> {
        let concept = self.concept(concept_id)?;
        if !concept.active {
            return Err(Error::domain(
                "concept_retired",
                format!("concept {concept_id} is retired"),
            ));
        }
        Ok(concept)
    }

    fn active_concept_mut(&mut self, concept_id: &str) -> Result<&mut Concept> {
        let concept = self.concept_mut(concept_id)?;
        if !concept.active {
            return Err(Error::domain(
                "concept_retired",
                format!("concept {concept_id} is retired"),
            ));
        }
        Ok(concept)
    }

    fn concept(&self, concept_id: &str) -> Result<&Concept> {
        self.concepts.get(concept_id).ok_or_else(|| {
            Error::domain(
                "concept_not_found",
                format!("concept {concept_id} does not exist"),
            )
        })
    }

    fn concept_mut(&mut self, concept_id: &str) -> Result<&mut Concept> {
        self.concepts.get_mut(concept_id).ok_or_else(|| {
            Error::domain(
                "concept_not_found",
                format!("concept {concept_id} does not exist"),
            )
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Revision {
    pub project_id: String,
    pub number: u64,
    pub summary: String,
    pub source_event_id: Option<String>,
    pub created_at: String,
    pub effects: Vec<SemanticEffect>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepositoryDiff {
    pub project_id: String,
    pub from_revision: u64,
    pub to_revision: u64,
    pub revisions: Vec<Revision>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntakeStatus {
    Unassigned,
    Pending,
    AwaitingReview,
    Paused,
    Processing,
    Applied,
    Ignored,
    Failed,
}

impl IntakeStatus {
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Unassigned => "unassigned",
            Self::Pending => "pending",
            Self::AwaitingReview => "awaiting_review",
            Self::Paused => "paused",
            Self::Processing => "processing",
            Self::Applied => "applied",
            Self::Ignored => "ignored",
            Self::Failed => "failed",
        }
    }
}

impl FromStr for IntakeStatus {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "unassigned" => Ok(Self::Unassigned),
            "pending" => Ok(Self::Pending),
            "awaiting_review" => Ok(Self::AwaitingReview),
            "paused" => Ok(Self::Paused),
            "processing" => Ok(Self::Processing),
            "applied" => Ok(Self::Applied),
            "ignored" => Ok(Self::Ignored),
            "failed" => Ok(Self::Failed),
            _ => Err(Error::domain(
                "intake_status_invalid",
                format!("unknown intake status {value:?}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct DecisionAnchor {
    pub source_role: String,
    pub host_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub message_role: String,
    pub occurred_at: i64,
    pub timestamp_precision: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct DecisionEvent {
    pub event_id: String,
    pub event_version: u32,
    pub cursor: String,
    pub event_kind: String,
    pub occurred_at: i64,
    pub decision_id: String,
    pub decided_at: i64,
    pub timestamp_precision: String,
    pub statement: String,
    pub disposition: String,
    pub confidence: String,
    pub rationale: Option<String>,
    pub supersedes_decision_id: Option<String>,
    pub authority_start: i64,
    pub authority_end: i64,
    pub review_state: String,
    pub review_id: Option<String>,
    pub review_action: Option<String>,
    pub reviewed_at: Option<i64>,
    pub review_source: Option<String>,
    pub anchors: Vec<DecisionAnchor>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct DecisionAccountAnchor {
    pub host_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub span_start: u64,
    pub span_end: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct DecisionAccountEvent {
    pub library_id: String,
    pub cursor: String,
    pub event_id: String,
    pub account_id: String,
    pub account_schema_version: u32,
    pub statement: String,
    pub context: String,
    pub action: String,
    pub result: String,
    pub occurred_at: i64,
    pub occurred_at_precision: String,
    pub authority: DecisionAccountAnchor,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountRoutingOutcome {
    ProjectAssigned,
    CwdMissing,
    CwdUnavailable,
    ProjectAmbiguous,
}

impl AccountRoutingOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProjectAssigned => "project_assigned",
            Self::CwdMissing => "cwd_missing",
            Self::CwdUnavailable => "cwd_unavailable",
            Self::ProjectAmbiguous => "project_ambiguous",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "project_assigned" => Ok(Self::ProjectAssigned),
            "cwd_missing" => Ok(Self::CwdMissing),
            "cwd_unavailable" => Ok(Self::CwdUnavailable),
            "project_ambiguous" => Ok(Self::ProjectAmbiguous),
            _ => Err(Error::domain(
                "account_routing_outcome_invalid",
                "stored account routing outcome is invalid",
            )),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AccountIntake {
    pub event_id: String,
    pub source_cursor: String,
    pub project_id: Option<String>,
    pub status: IntakeStatus,
    pub routing_outcome: AccountRoutingOutcome,
    pub account: DecisionAccountEvent,
    pub attempts: u64,
    pub last_error: Option<String>,
    pub terminal_reason: Option<String>,
    pub applied_revision: Option<u64>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Intake {
    pub event_id: String,
    pub source_cursor: String,
    pub project_id: Option<String>,
    pub status: IntakeStatus,
    pub cwd: Option<String>,
    pub decision: DecisionEvent,
    pub attempts: u64,
    pub last_error: Option<String>,
    pub terminal_reason: Option<String>,
    pub applied_revision: Option<u64>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReconciliationProposal {
    pub base_revision: u64,
    pub summary: String,
    pub effects: Vec<SemanticEffect>,
}

pub fn validate_project_id(project_id: &str) -> Result<()> {
    let valid = !project_id.is_empty()
        && project_id.len() <= 64
        && project_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && project_id
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_lowercase);
    if !valid {
        return Err(Error::domain(
            "project_id_invalid",
            "project IDs must start with a lowercase letter and contain only lowercase ASCII letters, digits, and '-'",
        ));
    }
    Ok(())
}

pub(crate) fn validate_annals_library_id(library_id: &str) -> Result<()> {
    let valid = library_id.len() == 32
        && library_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !valid {
        return Err(Error::domain(
            "annals_library_invalid",
            "Annals library identity must be exactly 32 lowercase hexadecimal characters",
        ));
    }
    Ok(())
}

pub fn marker_for(project_id: &str) -> String {
    format!("{PARTICIPATION_MARKER_PREFIX}{project_id}")
}

pub fn validate_effects_for_next_ids(
    repository: &Repository,
    effects: &[SemanticEffect],
    next_concept_number: u64,
) -> Result<u64> {
    let mut expected = next_concept_number;
    for effect in effects {
        if let SemanticEffect::Define { concept_id, .. } = effect {
            let wanted = concept_id_for(expected);
            if concept_id != &wanted {
                return Err(Error::domain(
                    "concept_id_out_of_sequence",
                    format!("next defined concept must be {wanted}, received {concept_id}"),
                ));
            }
            expected += 1;
        }
    }
    let mut candidate = repository.clone();
    candidate.apply_revision(repository.revision + 1, effects)?;
    Ok(expected)
}

#[must_use]
pub fn concept_id_for(number: u64) -> String {
    format!("c{number:06}")
}

#[must_use]
pub fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn validate_effect_count(effects: &[SemanticEffect]) -> Result<()> {
    if effects.is_empty() || effects.len() > MAX_EFFECTS {
        return Err(Error::domain(
            "effects_invalid",
            format!("a revision requires 1..={MAX_EFFECTS} effects"),
        ));
    }
    Ok(())
}

fn validate_concept_id(value: &str) -> Result<()> {
    let valid = value.len() == 7
        && value.starts_with('c')
        && value[1..].bytes().all(|byte| byte.is_ascii_digit());
    if !valid {
        return Err(Error::domain(
            "concept_id_invalid",
            format!("invalid stable concept ID {value:?}"),
        ));
    }
    Ok(())
}

fn validate_text(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > MAX_TEXT {
        return Err(Error::domain(
            "semantic_text_invalid",
            format!("{field} must contain 1..={MAX_TEXT} bytes"),
        ));
    }
    Ok(())
}

fn validate_grounding_source(source: &GroundingSource) -> Result<()> {
    match source {
        GroundingSource::Decision {
            event_id,
            decision_id,
        } => {
            validate_text("event_id", event_id)?;
            validate_text("decision_id", decision_id)
        }
        GroundingSource::AnnalsDecisionAccount {
            library_id,
            event_id,
            account_id,
        } => {
            validate_annals_library_id(library_id)?;
            validate_text("event_id", event_id)?;
            validate_text("account_id", account_id)
        }
        GroundingSource::Seed {
            source_label,
            digest,
        } => {
            validate_text("source_label", source_label)?;
            if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(Error::domain(
                    "seed_digest_invalid",
                    "seed digest must be a 64-character hexadecimal SHA-256 digest",
                ));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GroundingSource, Repository, SemanticEffect, concept_id_for, validate_effects_for_next_ids,
    };

    #[test]
    fn replay_enforces_stable_sequential_concepts_and_lifecycle() {
        let mut repository = Repository::empty("cell");
        let define = SemanticEffect::Define {
            concept_id: concept_id_for(1),
            label: "Concern".to_owned(),
            meaning: "A durable open question.".to_owned(),
        };
        assert_eq!(
            validate_effects_for_next_ids(&repository, std::slice::from_ref(&define), 1)
                .expect("valid definition"),
            2
        );
        repository
            .apply_revision(1, &[define])
            .expect("valid revision");
        repository
            .apply_revision(
                2,
                &[
                    SemanticEffect::Ground {
                        concept_id: "c000001".to_owned(),
                        source: GroundingSource::Decision {
                            event_id: "event-1".to_owned(),
                            decision_id: "decision-1".to_owned(),
                        },
                        statement: "Concerns remain open until settled.".to_owned(),
                    },
                    SemanticEffect::Retire {
                        concept_id: "c000001".to_owned(),
                        reason: "Superseded.".to_owned(),
                        replacement_concept_id: None,
                    },
                ],
            )
            .expect("valid lifecycle revision");
        assert!(!repository.concepts["c000001"].active);
        assert_eq!(repository.concepts["c000001"].grounds.len(), 1);
    }

    #[test]
    fn invalid_cross_reference_rejects_whole_revision_candidate() {
        let mut repository = Repository::empty("cell");
        let error = repository
            .apply_revision(
                1,
                &[SemanticEffect::Differentiate {
                    concept_id: "c000001".to_owned(),
                    other_concept_id: "c000002".to_owned(),
                    distinction: "Different scopes.".to_owned(),
                }],
            )
            .expect_err("missing concepts must fail");
        assert_eq!(error.code(), "concept_not_found");
        assert_eq!(repository.revision, 0);
    }

    #[test]
    fn failed_multi_effect_revision_is_atomic() {
        let mut repository = Repository::empty("cell");
        let error = repository
            .apply_revision(
                1,
                &[
                    SemanticEffect::Define {
                        concept_id: "c000001".to_owned(),
                        label: "Concern".to_owned(),
                        meaning: "An open question.".to_owned(),
                    },
                    SemanticEffect::Differentiate {
                        concept_id: "c000001".to_owned(),
                        other_concept_id: "c000002".to_owned(),
                        distinction: "Different scope.".to_owned(),
                    },
                ],
            )
            .expect_err("invalid tail effect must reject the whole revision");
        assert_eq!(error.code(), "concept_not_found");
        assert!(repository.concepts.is_empty());
        assert_eq!(repository.revision, 0);
    }

    #[test]
    fn active_labels_are_unique_after_normalization() {
        let mut repository = Repository::empty("cell");
        let error = repository
            .apply_revision(
                1,
                &[
                    SemanticEffect::Define {
                        concept_id: "c000001".to_owned(),
                        label: "Semantic   Concern".to_owned(),
                        meaning: "One.".to_owned(),
                    },
                    SemanticEffect::Define {
                        concept_id: "c000002".to_owned(),
                        label: " semantic concern ".to_owned(),
                        meaning: "Two.".to_owned(),
                    },
                ],
            )
            .expect_err("normalized duplicate must fail");
        assert_eq!(error.code(), "active_label_duplicate");
        assert!(repository.concepts.is_empty());
    }

    #[test]
    fn retired_concept_cannot_receive_new_grounding() {
        let mut repository = Repository::empty("cell");
        repository
            .apply_revision(
                1,
                &[SemanticEffect::Define {
                    concept_id: "c000001".to_owned(),
                    label: "Concern".to_owned(),
                    meaning: "One.".to_owned(),
                }],
            )
            .expect("definition");
        repository
            .apply_revision(
                2,
                &[SemanticEffect::Retire {
                    concept_id: "c000001".to_owned(),
                    reason: "No longer current.".to_owned(),
                    replacement_concept_id: None,
                }],
            )
            .expect("retirement");
        let error = repository
            .apply_revision(
                3,
                &[SemanticEffect::Ground {
                    concept_id: "c000001".to_owned(),
                    source: GroundingSource::Decision {
                        event_id: "event-2".to_owned(),
                        decision_id: "decision-2".to_owned(),
                    },
                    statement: "Still current.".to_owned(),
                }],
            )
            .expect_err("retired concepts require reopen before grounding");
        assert_eq!(error.code(), "concept_retired");
        assert_eq!(repository.revision, 2);
    }

    #[test]
    fn unground_preserves_withdrawn_evidence_history() {
        let mut repository = Repository::empty("cell");
        repository
            .apply_revision(
                1,
                &[
                    SemanticEffect::Define {
                        concept_id: "c000001".to_owned(),
                        label: "Concern".to_owned(),
                        meaning: "One.".to_owned(),
                    },
                    SemanticEffect::Ground {
                        concept_id: "c000001".to_owned(),
                        source: GroundingSource::Decision {
                            event_id: "event-1".to_owned(),
                            decision_id: "decision-1".to_owned(),
                        },
                        statement: "Use this term.".to_owned(),
                    },
                ],
            )
            .expect("grounding");
        repository
            .apply_revision(
                2,
                &[SemanticEffect::Unground {
                    concept_id: "c000001".to_owned(),
                    event_id: "event-1".to_owned(),
                    decision_id: "decision-1".to_owned(),
                    withdrawal_event_id: "review-1".to_owned(),
                    reason: "Decision dismissed.".to_owned(),
                }],
            )
            .expect("withdrawal");
        let grounding = &repository.concepts["c000001"].grounds[0];
        assert!(!grounding.active);
        assert_eq!(grounding.withdrawn_revision, Some(2));
        assert_eq!(grounding.withdrawal_event_id.as_deref(), Some("review-1"));
    }
}
