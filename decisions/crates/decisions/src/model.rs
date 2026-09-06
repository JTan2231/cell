use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Disposition {
    Adopt,
    Reject,
    Forbid,
    Defer,
    Delegate,
    Reopen,
    Supersede,
}

impl Disposition {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Adopt => "adopt",
            Self::Reject => "reject",
            Self::Forbid => "forbid",
            Self::Defer => "defer",
            Self::Delegate => "delegate",
            Self::Reopen => "reopen",
            Self::Supersede => "supersede",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Precision {
    Item,
    Turn,
}

impl Precision {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Turn => "turn",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SourceMessage {
    pub(crate) host_id: String,
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
    pub(crate) item_id: String,
    pub(crate) role: MessageRole,
    pub(crate) text: String,
    pub(crate) occurred_at: i64,
    pub(crate) precision: Precision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MessageRole {
    User,
    Assistant,
}

impl MessageRole {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ThreadTranscript {
    pub(crate) host_id: String,
    pub(crate) thread_id: String,
    pub(crate) messages: Vec<SourceMessage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubmittedClassification {
    pub(crate) decisions: Vec<SubmittedCandidate>,
    pub(crate) complete: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubmittedObservationClassification {
    pub(crate) verdicts: Vec<SubmittedAuthorityVerdict>,
    pub(crate) needs_context: bool,
    pub(crate) complete: bool,
}

/// The only model-authored positive payload in the active Krisis contract.
///
/// Source aliases are resolved to private local anchors after validation; the
/// model never sees or supplies real host, thread, turn, or item identifiers.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubmittedDecisionAccount {
    pub(crate) authority_source_id: String,
    pub(crate) authority_quote: String,
    #[serde(default)]
    pub(crate) context_source_ids: Vec<String>,
    #[serde(default)]
    pub(crate) action_source_ids: Vec<String>,
    #[serde(default)]
    pub(crate) result_source_ids: Vec<String>,
    pub(crate) statement: String,
    pub(crate) context: Option<String>,
    pub(crate) action: Option<String>,
    pub(crate) result: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubmittedAccountClassification {
    pub(crate) verdicts: Vec<SubmittedAccountVerdict>,
    pub(crate) needs_context: bool,
    pub(crate) complete: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubmittedAccountVerdict {
    pub(crate) authority_source_id: String,
    pub(crate) verdict: AuthorityVerdict,
    #[serde(default)]
    pub(crate) accounts: Vec<SubmittedDecisionAccount>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubmittedAuthorityVerdict {
    pub(crate) authority_source_id: String,
    pub(crate) verdict: AuthorityVerdict,
    #[serde(default)]
    pub(crate) decisions: Vec<SubmittedCandidate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthorityVerdict {
    Decision,
    NoDecision,
}

impl AuthorityVerdict {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::NoDecision => "no_decision",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AuthorityMessageVerdict {
    pub(crate) authority: SourceMessage,
    pub(crate) verdict: AuthorityVerdict,
}

#[derive(Debug, Clone)]
pub(crate) struct ObservationClassification {
    pub(crate) accounts: Vec<DecisionAccount>,
    /// Version-two/three receipt payload retained only for upgrade recovery.
    pub(crate) candidates: Vec<Candidate>,
    pub(crate) authority_verdicts: Vec<AuthorityMessageVerdict>,
    pub(crate) needs_context: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AccountSource {
    pub(crate) host_id: String,
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
    pub(crate) item_id: String,
    pub(crate) role: MessageRole,
    pub(crate) occurred_at: i64,
    pub(crate) precision: Precision,
}

impl AccountSource {
    pub(crate) fn from_source(source: &SourceMessage) -> Self {
        Self {
            host_id: source.host_id.clone(),
            thread_id: source.thread_id.clone(),
            turn_id: source.turn_id.clone(),
            item_id: source.item_id.clone(),
            role: source.role,
            occurred_at: source.occurred_at,
            precision: source.precision,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DecisionAccount {
    pub(crate) id: String,
    pub(crate) occurred_at: i64,
    pub(crate) precision: Precision,
    pub(crate) statement: String,
    pub(crate) authority_quote: String,
    pub(crate) context: Option<String>,
    pub(crate) action: Option<String>,
    pub(crate) result: Option<String>,
    pub(crate) authority_start: usize,
    pub(crate) authority_end: usize,
    pub(crate) authority: AccountSource,
    pub(crate) context_sources: Vec<AccountSource>,
    pub(crate) action_sources: Vec<AccountSource>,
    pub(crate) result_sources: Vec<AccountSource>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubmittedCandidate {
    pub(crate) authority_source_id: String,
    pub(crate) authority_excerpt: String,
    #[serde(default)]
    pub(crate) context_source_ids: Vec<String>,
    pub(crate) statement: String,
    pub(crate) disposition: Disposition,
    pub(crate) confidence: Confidence,
    #[serde(default)]
    pub(crate) rationale: Option<String>,
    #[serde(default)]
    pub(crate) supersedes_decision_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub(crate) id: String,
    pub(crate) decided_at: i64,
    pub(crate) precision: Precision,
    pub(crate) statement: String,
    pub(crate) disposition: Disposition,
    pub(crate) confidence: Confidence,
    pub(crate) rationale: Option<String>,
    pub(crate) supersedes_id: Option<String>,
    pub(crate) authority_start: usize,
    pub(crate) authority_end: usize,
    pub(crate) authority: SourceMessage,
    pub(crate) context: Vec<SourceMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedSource {
    pub(crate) host_id: String,
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
    pub(crate) item_id: String,
    pub(crate) role: MessageRole,
    pub(crate) occurred_at: i64,
    pub(crate) precision: Precision,
}

impl PersistedSource {
    fn from_source(source: &SourceMessage) -> Self {
        Self {
            host_id: source.host_id.clone(),
            thread_id: source.thread_id.clone(),
            turn_id: source.turn_id.clone(),
            item_id: source.item_id.clone(),
            role: source.role,
            occurred_at: source.occurred_at,
            precision: source.precision,
        }
    }

    fn into_source(self) -> SourceMessage {
        SourceMessage {
            host_id: self.host_id,
            thread_id: self.thread_id,
            turn_id: self.turn_id,
            item_id: self.item_id,
            role: self.role,
            text: String::new(),
            occurred_at: self.occurred_at,
            precision: self.precision,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedCandidate {
    pub(crate) id: String,
    pub(crate) decided_at: i64,
    pub(crate) precision: Precision,
    pub(crate) statement: String,
    pub(crate) disposition: Disposition,
    pub(crate) confidence: Confidence,
    pub(crate) rationale: Option<String>,
    pub(crate) supersedes_id: Option<String>,
    pub(crate) authority_start: usize,
    pub(crate) authority_end: usize,
    pub(crate) authority: PersistedSource,
    pub(crate) context: Vec<PersistedSource>,
}

impl PersistedCandidate {
    pub(crate) fn from_candidate(candidate: &Candidate) -> Self {
        Self {
            id: candidate.id.clone(),
            decided_at: candidate.decided_at,
            precision: candidate.precision,
            statement: candidate.statement.clone(),
            disposition: candidate.disposition,
            confidence: candidate.confidence,
            rationale: candidate.rationale.clone(),
            supersedes_id: candidate.supersedes_id.clone(),
            authority_start: candidate.authority_start,
            authority_end: candidate.authority_end,
            authority: PersistedSource::from_source(&candidate.authority),
            context: candidate
                .context
                .iter()
                .map(PersistedSource::from_source)
                .collect(),
        }
    }

    pub(crate) fn into_candidate(self) -> Candidate {
        Candidate {
            id: self.id,
            decided_at: self.decided_at,
            precision: self.precision,
            statement: self.statement,
            disposition: self.disposition,
            confidence: self.confidence,
            rationale: self.rationale,
            supersedes_id: self.supersedes_id,
            authority_start: self.authority_start,
            authority_end: self.authority_end,
            authority: self.authority.into_source(),
            context: self
                .context
                .into_iter()
                .map(PersistedSource::into_source)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedAuthorityVerdict {
    authority: PersistedSource,
    verdict: AuthorityVerdict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedObservationClassification {
    #[serde(default)]
    accounts: Vec<DecisionAccount>,
    #[serde(default)]
    candidates: Vec<PersistedCandidate>,
    authority_verdicts: Vec<PersistedAuthorityVerdict>,
    needs_context: bool,
}

impl PersistedObservationClassification {
    pub(crate) fn from_classification(classification: &ObservationClassification) -> Self {
        Self {
            accounts: classification.accounts.clone(),
            candidates: classification
                .candidates
                .iter()
                .map(PersistedCandidate::from_candidate)
                .collect(),
            authority_verdicts: classification
                .authority_verdicts
                .iter()
                .map(|verdict| PersistedAuthorityVerdict {
                    authority: PersistedSource::from_source(&verdict.authority),
                    verdict: verdict.verdict,
                })
                .collect(),
            needs_context: classification.needs_context,
        }
    }

    pub(crate) fn into_classification(self) -> ObservationClassification {
        ObservationClassification {
            accounts: self.accounts,
            candidates: self
                .candidates
                .into_iter()
                .map(PersistedCandidate::into_candidate)
                .collect(),
            authority_verdicts: self
                .authority_verdicts
                .into_iter()
                .map(|verdict| AuthorityMessageVerdict {
                    authority: verdict.authority.into_source(),
                    verdict: verdict.verdict,
                })
                .collect(),
            needs_context: self.needs_context,
        }
    }
}

pub(crate) use decisions::api::StoredCandidate;

pub(crate) use krisis_api::lifecycle::{
    DecisionEventAuthoritySpan, DecisionEventDecision, DecisionEventEnvelope, DecisionEventReview,
    DecisionEventSource as StoredSource,
};

#[derive(Debug, Clone)]
pub(crate) struct Run {
    pub(crate) id: String,
    pub(crate) report_date: String,
    pub(crate) status: String,
    pub(crate) content_revision: i64,
}

pub(crate) use decisions::api::{Observation, ObservationFailure, ObservationStatus};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DigestSnapshot {
    pub(crate) run_id: String,
    pub(crate) report_date: String,
    pub(crate) content_revision: i64,
    pub(crate) subject: String,
    pub(crate) body: String,
    pub(crate) digest_hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Delivery {
    pub(crate) id: String,
    pub(crate) run_id: String,
    pub(crate) kind: String,
    pub(crate) occurrence_date: Option<String>,
    pub(crate) idempotency_key: String,
    pub(crate) status: String,
    pub(crate) email_id: Option<String>,
}
