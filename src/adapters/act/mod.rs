//! Vertikaler Adaptervertrag für kontrollierte, idempotente externe Wirkungen.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::{AdapterCallError, AdapterManifest, CapabilityRef};
use crate::core::{SchemaId, VersionId};

/// Vom Core erzeugter Auftrag aus einem materialisierten Action-Artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActInvocation {
    pub invocation_id: String,
    pub capability: CapabilityRef,
    pub action_version_id: VersionId,
    pub action_schema_id: SchemaId,
    pub payload: Value,
    pub result_schema_id: SchemaId,
}

/// Korrelierte, weiterhin untrusted Rückgabe eines Act-Adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActResponse {
    pub invocation_id: String,
    pub external_reference: String,
    pub result_payload: Value,
}

/// Port zu einem externen, idempotenten Effekt-Executor.
pub trait ActAdapter: Send + Sync {
    fn manifest(&self) -> &AdapterManifest;

    fn execute(&self, invocation: &ActInvocation) -> Result<ActResponse, AdapterCallError>;
}
