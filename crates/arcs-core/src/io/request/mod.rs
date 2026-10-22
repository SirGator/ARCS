//! Vertikaler Slice für gezielt angeforderte externe Daten.
//!
//! Ein Request beginnt immer im ARCS-Core und bezieht sich auf ein bereits
//! persistiertes Request-Artifact. Der externe Microservice liefert nur eine
//! korrelierte Referenz und JSON-Payload zurück. Identität, Trust, Subject,
//! Provenienz und Relationen bleiben unter Kontrolle des Cores.

use crate::adapters::{AdapterCallError, AdapterManifest};
pub use arcs_adapter_protocol::{RequestInvocation, RequestResponse};

mod error;
mod service;
pub use error::RequestError;
pub use service::RequestService;

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
