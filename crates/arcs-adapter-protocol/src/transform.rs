use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CapabilityRef, SchemaId, VersionId};

/// Kontrollierter Auftrag zur deterministischen Verarbeitung eines Artefakts.
/// Die Daten dürfen keine externen Wirkungen oder Freigaben auslösen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformInvocation {
    pub invocation_id: String,
    pub capability: CapabilityRef,
    pub input_version_id: VersionId,
    pub input_schema_id: SchemaId,
    pub input_payload: Value,
    pub output_schema_id: SchemaId,
    pub max_output_bytes: usize,
}

/// Der Adapter liefert nur Payload und Korrelation; Metadaten setzt der Core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformResponse {
    pub invocation_id: String,
    pub external_reference: String,
    pub payload: Value,
}
