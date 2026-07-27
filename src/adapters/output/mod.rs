//! Vertikaler Adaptervertrag für korrelierte externe Ausgabe.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::port::AdapterCallError;
use crate::adapters::registration::{AdapterManifest, CapabilityRef};
use crate::core::{SchemaId, SubjectId, VersionId};

/// Vom Core erzeugter Ausgabeauftrag.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputInvocation {
    pub invocation_id: String,
    /// Exakt die vom Core autorisierte Ausgabefähigkeit dieses Auftrags.
    pub capability: CapabilityRef,
    pub artifact_version_id: VersionId,
    pub artifact_schema_id: SchemaId,
    pub subject: Option<SubjectId>,
    pub payload: Value,
    pub result_schema_id: SchemaId,
}

/// Korrelierte Empfangsbestätigung des Output-Adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputResponse {
    pub invocation_id: String,
    pub external_reference: String,
    pub result_payload: Value,
}

/// Interner Port zu Chat, UI, Datei, API oder einer anderen Ausgabesenke.
pub trait OutputAdapter: Send + Sync {
    fn manifest(&self) -> &AdapterManifest;

    fn deliver(&self, request: &OutputInvocation) -> Result<OutputResponse, AdapterCallError>;
}
