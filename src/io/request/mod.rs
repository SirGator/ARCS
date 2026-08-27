//! Vertikaler Slice für gezielt angeforderte externe Daten.
//!
//! Ein Request beginnt immer im ARCS-Core und bezieht sich auf ein bereits
//! persistiertes Request-Artifact. Der externe Microservice liefert nur eine
//! korrelierte Referenz und JSON-Payload zurück. Identität, Trust, Subject,
//! Provenienz und Relationen bleiben unter Kontrolle des Cores.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::port::AdapterCallError;
use crate::adapters::registration::{AdapterManifest, CapabilityRef};
use crate::core::{SchemaId, SubjectId, VersionId};

mod error;
mod service;

pub use error::RequestError;
pub use service::RequestService;

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

/// Port zu einem externen API-, Datenbank-, Datei- oder System-Query-Service.
///
/// Der konkrete Transport kann HTTP, stdio oder IPC sein. Der Service erhält
/// keinen Zugriff auf Store, Registry oder andere ARCS-Komponenten.
pub trait RequestAdapter: Send + Sync {
    fn manifest(&self) -> &AdapterManifest;

    fn fetch(&self, request: &RequestInvocation) -> Result<RequestResponse, AdapterCallError>;
}

#[cfg(test)]
mod tests;
