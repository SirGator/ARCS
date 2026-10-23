//! Kontrollierte, deterministische Ableitungen aus persistierten Artefakten.

use crate::adapters::{AdapterCallError, AdapterManifest};
pub use arcs_adapter_protocol::{TransformInvocation, TransformResponse};

mod error;
mod service;
pub use error::TransformError;
pub use service::TransformService;

/// Ein Transform verarbeitet Daten ohne externe Effekte. Identische Aufrufe
/// müssen dasselbe Ergebnis liefern, auch nach verlorenen Antworten.
pub trait TransformAdapter: Send + Sync {
    fn manifest(&self) -> &AdapterManifest;
    fn transform(
        &self,
        invocation: &TransformInvocation,
    ) -> Result<TransformResponse, AdapterCallError>;
}

#[cfg(test)]
mod tests;
