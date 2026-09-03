use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub(crate) const OUTPUT_SCHEMA_VERSION: u32 = 2;
pub(crate) const PROVIDER_SCHEMA_VERSION: u32 = 3;
pub(crate) const PREVIOUS_PROVIDER_SCHEMA_VERSION: u32 = 2;
pub(crate) const LEGACY_PROVIDER_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderManifest {
    pub(crate) schema_version: u32,
    pub(crate) provider: ProviderIdentity,
    #[serde(default)]
    pub(crate) promise_scope: Option<ProviderPromiseScope>,
    #[serde(skip)]
    pub(crate) promise_scope_present: bool,
    pub(crate) entries: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderPromiseScope {
    pub(crate) authoritative_for: Vec<String>,
    pub(crate) not_authoritative_for: Vec<String>,
    pub(crate) inventory: InventoryScope,
    pub(crate) shared_access_and_trust: Vec<String>,
    pub(crate) shared_privacy_and_retention: Vec<String>,
    pub(crate) compatibility_and_retirement: Vec<String>,
    pub(crate) operational_limits: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InventoryScope {
    pub(crate) covers: Vec<String>,
    pub(crate) completeness: InventoryCompleteness,
    pub(crate) excludes: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InventoryCompleteness {
    Complete,
    Partial,
}

impl InventoryCompleteness {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderIdentity {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) release: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EntryKind {
    Capability,
    Operation,
}

impl EntryKind {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Capability => "capability",
            Self::Operation => "operation",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Mode {
    Use,
    Operate,
    Develop,
}

impl Mode {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Use => "use",
            Self::Operate => "operate",
            Self::Develop => "develop",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Support {
    Supported,
    Deprecated,
}

impl Support {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Deprecated => "deprecated",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Interface {
    pub(crate) label: String,
    pub(crate) invocation: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Dependency {
    pub(crate) id: String,
    pub(crate) min_contract: u32,
    pub(crate) max_contract_exclusive: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ClaimStatus {
    Declared,
    Unsupported,
    Unspecified,
    NotApplicable,
}

impl ClaimStatus {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Declared => "declared",
            Self::Unsupported => "unsupported",
            Self::Unspecified => "unspecified",
            Self::NotApplicable => "not_applicable",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PromiseClaim {
    pub(crate) status: ClaimStatus,
    pub(crate) statement: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RelianceKind {
    Data,
    Control,
    Authority,
    Readiness,
    External,
}

impl RelianceKind {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Control => "control",
            Self::Authority => "authority",
            Self::Readiness => "readiness",
            Self::External => "external",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RelianceClaim {
    pub(crate) status: ClaimStatus,
    pub(crate) statement: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) kind: Option<RelianceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) contract: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntryPromise {
    pub(crate) consumers: Vec<PromiseClaim>,
    pub(crate) preconditions: Vec<PromiseClaim>,
    pub(crate) inputs: Vec<PromiseClaim>,
    pub(crate) outputs: Vec<PromiseClaim>,
    pub(crate) data_semantics: Vec<PromiseClaim>,
    pub(crate) identity_and_units: Vec<PromiseClaim>,
    pub(crate) completeness_and_freshness: Vec<PromiseClaim>,
    pub(crate) access: Vec<PromiseClaim>,
    pub(crate) lifecycle_and_consistency: Vec<PromiseClaim>,
    pub(crate) operational_limits: Vec<PromiseClaim>,
    pub(crate) compatibility_and_evolution: Vec<PromiseClaim>,
    pub(crate) reliances: Vec<RelianceClaim>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PromiseFacet {
    Applicability,
    Outcome,
    Consumers,
    Preconditions,
    Interfaces,
    Inputs,
    Outputs,
    DataSemantics,
    IdentityAndUnits,
    CompletenessAndFreshness,
    Effects,
    Authority,
    Access,
    LifecycleAndConsistency,
    Success,
    FailureAndRecovery,
    Privacy,
    OperationalLimits,
    CompatibilityAndEvolution,
    Dependencies,
    Reliances,
    Exclusions,
}

impl PromiseFacet {
    pub(crate) const ALL: [Self; 22] = [
        Self::Applicability,
        Self::Outcome,
        Self::Consumers,
        Self::Preconditions,
        Self::Interfaces,
        Self::Inputs,
        Self::Outputs,
        Self::DataSemantics,
        Self::IdentityAndUnits,
        Self::CompletenessAndFreshness,
        Self::Effects,
        Self::Authority,
        Self::Access,
        Self::LifecycleAndConsistency,
        Self::Success,
        Self::FailureAndRecovery,
        Self::Privacy,
        Self::OperationalLimits,
        Self::CompatibilityAndEvolution,
        Self::Dependencies,
        Self::Reliances,
        Self::Exclusions,
    ];

    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Applicability => "applicability",
            Self::Outcome => "outcome",
            Self::Consumers => "consumers",
            Self::Preconditions => "preconditions",
            Self::Interfaces => "interfaces",
            Self::Inputs => "inputs",
            Self::Outputs => "outputs",
            Self::DataSemantics => "data_semantics",
            Self::IdentityAndUnits => "identity_and_units",
            Self::CompletenessAndFreshness => "completeness_and_freshness",
            Self::Effects => "effects",
            Self::Authority => "authority",
            Self::Access => "access",
            Self::LifecycleAndConsistency => "lifecycle_and_consistency",
            Self::Success => "success",
            Self::FailureAndRecovery => "failure_and_recovery",
            Self::Privacy => "privacy",
            Self::OperationalLimits => "operational_limits",
            Self::CompatibilityAndEvolution => "compatibility_and_evolution",
            Self::Dependencies => "dependencies",
            Self::Reliances => "reliances",
            Self::Exclusions => "exclusions",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntryDocument {
    pub(crate) id: String,
    pub(crate) contract_version: u32,
    pub(crate) kind: EntryKind,
    pub(crate) mode: Mode,
    pub(crate) support: Support,
    pub(crate) title: String,
    pub(crate) summary: String,
    pub(crate) use_when: Vec<String>,
    pub(crate) do_not_use_when: Vec<String>,
    pub(crate) outcome: String,
    pub(crate) effects: Vec<String>,
    pub(crate) authority: Vec<String>,
    pub(crate) success: Vec<String>,
    pub(crate) failure_and_recovery: Vec<String>,
    pub(crate) privacy: Vec<String>,
    #[serde(default)]
    pub(crate) interfaces: Vec<Interface>,
    #[serde(default)]
    pub(crate) dependencies: Vec<Dependency>,
    #[serde(default)]
    pub(crate) promise: Option<EntryPromise>,
    #[serde(skip)]
    pub(crate) promise_present: bool,
    #[serde(default)]
    pub(crate) session_surfaces: Vec<String>,
    #[serde(default)]
    pub(crate) does_not_authorize: Vec<String>,
    #[serde(default)]
    pub(crate) runtime: Option<String>,
    #[serde(default)]
    pub(crate) automation: Option<String>,
    #[serde(default)]
    pub(crate) steps: Vec<String>,
    #[serde(default)]
    pub(crate) checkpoints: Vec<String>,
    #[serde(default)]
    pub(crate) adaptation: Vec<String>,
    #[serde(default)]
    pub(crate) stop_when: Vec<String>,
    pub(crate) manual: String,
}

#[derive(Debug, Clone)]
pub(crate) struct LoadedEntry {
    pub(crate) document: EntryDocument,
    pub(crate) source_path: String,
    pub(crate) source_sha256: String,
    pub(crate) manual_text: String,
    pub(crate) manual_sha256: String,
    pub(crate) dependency_statuses: Vec<DependencyStatus>,
    pub(crate) compatible: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DependencyStatus {
    pub(crate) id: String,
    pub(crate) min_contract: u32,
    pub(crate) max_contract_exclusive: u32,
    pub(crate) installed_contract: Option<u32>,
    pub(crate) state: DependencyState,
}

#[derive(Debug, Clone, Copy, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DependencyState {
    Compatible,
    Missing,
    Incompatible,
    Unavailable,
    Cycle,
}

impl DependencyState {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::Missing => "missing",
            Self::Incompatible => "incompatible",
            Self::Unavailable => "unavailable",
            Self::Cycle => "cycle",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderBundle {
    pub(crate) schema_version: u32,
    pub(crate) identity: ProviderIdentity,
    pub(crate) promise_scope: Option<ProviderPromiseScope>,
    pub(crate) root: PathBuf,
    pub(crate) manifest_sha256: String,
    pub(crate) entries: Vec<LoadedEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Issue {
    pub(crate) code: String,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) entry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) path: Option<String>,
}

impl Issue {
    pub(crate) fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            provider: None,
            entry: None,
            path: None,
        }
    }

    #[must_use]
    pub(crate) fn provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    #[must_use]
    pub(crate) fn entry(mut self, entry: impl Into<String>) -> Self {
        self.entry = Some(entry.into());
        self
    }

    #[must_use]
    pub(crate) fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }
}

#[derive(Debug)]
pub(crate) struct Registry {
    pub(crate) root: PathBuf,
    pub(crate) providers: Vec<ProviderBundle>,
    pub(crate) issues: Vec<Issue>,
    pub(crate) scanned_providers: usize,
}

impl Registry {
    pub(crate) fn entries(&self) -> impl Iterator<Item = (&ProviderIdentity, &LoadedEntry)> {
        self.providers.iter().flat_map(|provider| {
            provider
                .entries
                .iter()
                .map(move |entry| (&provider.identity, entry))
        })
    }

    #[must_use]
    pub(crate) fn entry_count(&self) -> usize {
        self.providers
            .iter()
            .map(|provider| provider.entries.len())
            .sum()
    }

    pub(crate) fn find_entry(&self, id: &str) -> Option<(&ProviderBundle, &LoadedEntry)> {
        self.providers.iter().find_map(|provider| {
            provider
                .entries
                .iter()
                .find(|entry| entry.document.id == id)
                .map(|entry| (provider, entry))
        })
    }
}
