use crate::{CapabilityRef, SchemaId, SubjectId, VersionId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Vom Core erzeugter, enger Auftrag an einen Request-Microservice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestInvocation {
    /// Stabile, ausschließlich vom Core erzeugte Korrelations-ID.
    pub invocation_id: String,
    /// Exakt die aufgelöste und freigeschaltete Fähigkeit.
    pub capability: CapabilityRef,
    /// Persistierte Version des auslösenden Request-Artifacts.
    pub request_version_id: VersionId,
    pub request_schema_id: SchemaId,
    pub subject: SubjectId,
    pub request_payload: Value,
    pub response_schema_id: SchemaId,
    /// Harte Obergrenze für die serialisierte Antwort.
    pub max_response_bytes: usize,
}

/// Nicht vertrauenswürdige, aber eindeutig korrelierte externe Antwort.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestResponse {
    pub invocation_id: String,
    pub external_reference: String,
    pub payload: Value,
}
