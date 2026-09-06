pub(crate) use clockwork::api::{
    ActivationRecord, ActivationState, DefinitionRecord, DefinitionSummary, LaunchImage, Manifest,
    Schedule, Trigger,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BindingRecord {
    pub(crate) key: String,
    pub(crate) definition_digest: Option<String>,
    pub(crate) enabled: bool,
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
            updated_at: record.updated_at,
        }
    }
}

#[cfg(test)]
pub(crate) use clockwork::api::{Authority, Output, OverlapPolicy};
