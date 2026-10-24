use crate::adapters::{AdapterId, AdapterRole, CapabilityRef, ProducerClass};
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
}
