pub(crate) use clockwork::api::{
    ActivationRecord, ActivationState, DefinitionRecord, DefinitionSummary, LaunchImage, Manifest,
    Schedule, Trigger,
};

#[derive(Debug, Clone)]
pub(crate) struct BindingRecord {
    pub(crate) key: String,
    pub(crate) definition_digest: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) halted_incident: Option<String>,
    pub(crate) failure_policy_active: bool,
    pub(crate) plist_sha256: Option<String>,
    pub(crate) updated_at: i64,
}

impl serde::Serialize for BindingRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&clockwork::api::BindingRecord::from(self), serializer)
    }
}

impl From<&BindingRecord> for clockwork::api::BindingRecord {
    fn from(record: &BindingRecord) -> Self {
        Self {
            key: record.key.clone(),
            definition_digest: record.definition_digest.clone(),
            enabled: record.enabled,
            halted_incident: record.halted_incident.clone(),
            failure_policy_active: record.failure_policy_active,
            updated_at: record.updated_at,
        }
    }
}

#[cfg(test)]
pub(crate) use clockwork::api::{Authority, Output, OverlapPolicy};

// A transition owns selection and launchd projection only. It never snapshots
// or restores failure state, which is retained independently in incidents.
impl PartialEq for BindingRecord {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.definition_digest == other.definition_digest
            && self.enabled == other.enabled
            && self.plist_sha256 == other.plist_sha256
            && self.updated_at == other.updated_at
    }
}
impl Eq for BindingRecord {}
