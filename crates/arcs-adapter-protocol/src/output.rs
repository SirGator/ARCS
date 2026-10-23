use crate::{CapabilityRef, SchemaId, SubjectId, VersionId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Vom Core erzeugter Ausgabeauftrag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputInvocation {
    pub invocation_id: String,
    /// Exakt die vom Core autorisierte Ausgabefähigkeit dieses Auftrags.
    pub capability: CapabilityRef,
    pub request_version_id: VersionId,
    pub approval_version_id: VersionId,
    pub artifact_version_id: VersionId,
    pub artifact_schema_id: SchemaId,
    pub subject: Option<SubjectId>,
    pub payload: Value,
    pub result_schema_id: SchemaId,
    pub max_result_bytes: usize,
}

/// Korrelierte Empfangsbestätigung des Output-Adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputResponse {
    pub invocation_id: String,
    pub external_reference: String,
    pub result_payload: Value,
}
