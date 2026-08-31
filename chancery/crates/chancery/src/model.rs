use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub(crate) const OUTPUT_SCHEMA_VERSION: u32 = 2;
pub(crate) const PROVIDER_SCHEMA_VERSION: u32 = 2;
pub(crate) const LEGACY_PROVIDER_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderManifest {
    pub(crate) schema_version: u32,
    pub(crate) provider: ProviderIdentity,
    pub(crate) entries: Vec<String>,
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
    pub(crate) manual_text: String,
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
    pub(crate) identity: ProviderIdentity,
    pub(crate) root: PathBuf,
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
}
