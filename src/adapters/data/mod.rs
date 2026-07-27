//! Vertikaler Adaptervertrag für korrelierte Datenbeschaffung.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::port::AdapterCallError;
use crate::adapters::registration::{AdapterManifest, CapabilityRef};
use crate::core::{SchemaId, SubjectId, VersionId};

/// Vom Core erzeugter, enger Auftrag an einen Data-Adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataInvocation {
    pub invocation_id: String,
    /// Exakt die vom Core aufgelöste und freigeschaltete Fähigkeit.
    ///
    /// Ein Adapterprozess kann mehrere Data-Capabilities anbieten. Die
    /// vollständige Referenz verhindert, dass der Endpoint den Auftrag aus
    /// Payload, Schema oder einem lokal mehrdeutigen Namen erraten muss.
    pub capability: CapabilityRef,
    pub request_version_id: VersionId,
    pub request_schema_id: SchemaId,
    pub subject: SubjectId,
    pub request_payload: Value,
    pub response_schema_id: SchemaId,
    pub max_response_bytes: usize,
}

/// Untrusted, aber eindeutig korrelierte Antwort eines Data-Adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataResponse {
    pub invocation_id: String,
    pub external_reference: String,
    pub payload: Value,
}

/// Interner Port zu einem externen Sensor-, API- oder System-Query-Adapter.
pub trait DataAdapter: Send + Sync {
    fn manifest(&self) -> &AdapterManifest;

    fn fetch(&self, request: &DataInvocation) -> Result<DataResponse, AdapterCallError>;
}
