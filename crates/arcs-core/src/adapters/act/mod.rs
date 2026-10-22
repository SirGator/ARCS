//! Vertikaler Adaptervertrag für kontrollierte, idempotente externe Wirkungen.

use crate::adapters::{AdapterCallError, AdapterManifest};
pub use arcs_adapter_protocol::{ActInvocation, ActResponse};

/// Port zu einem externen, idempotenten Effekt-Executor.
pub trait ActAdapter: Send + Sync {
    fn manifest(&self) -> &AdapterManifest;

    fn execute(&self, invocation: &ActInvocation) -> Result<ActResponse, AdapterCallError>;
}
