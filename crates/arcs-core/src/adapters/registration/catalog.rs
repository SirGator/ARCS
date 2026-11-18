use crate::adapters::{
    AdapterId, AdapterRole, CapabilityDescription, CapabilityRef, ProducerClass,
};
use crate::core::SchemaId;
use crate::reasoning::ReasoningMethod;
use serde::Serialize;

/// Beschreibung einer registrierten Installation. Der Katalog ersetzt weder
/// aktuelle Autorisierung noch Fehlerbehandlung beim tatsächlichen Aufruf.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AdapterConnection {
    pub adapter_id: AdapterId,
    pub adapter_version: String,
    pub producer_class: ProducerClass,
    pub capabilities: Vec<AvailableCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AvailableCapability {
    pub capability: CapabilityRef,
    pub role: AdapterRole,
    pub accepts: Vec<SchemaId>,
    pub emits: Vec<SchemaId>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reasoning_methods: Vec<ReasoningMethod>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description_claim: Option<CapabilityDescriptionClaim>,
}

/// Provenance is explicit, even for an adapter installed with System trust.
/// Structural registration checks do not verify these semantic claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CapabilityDescriptionClaim {
    pub source: DescriptionSource,
    pub verification: DescriptionVerification,
    pub description: CapabilityDescription,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DescriptionSource {
    Adapter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DescriptionVerification {
    Unverified,
}

impl CapabilityDescriptionClaim {
    pub(super) fn new(description: CapabilityDescription) -> Self {
        Self {
            source: DescriptionSource::Adapter,
            verification: DescriptionVerification::Unverified,
            description,
        }
    }
}
