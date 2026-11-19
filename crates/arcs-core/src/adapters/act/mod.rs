//! Vertikaler Adaptervertrag für kontrollierte, idempotente externe Wirkungen.

use crate::adapters::{AdapterCallError, AdapterManifest, CapabilityRef};
pub use arcs_adapter_protocol::{ActFence, ActInvocation, ActResponse};

/// Port zu einem externen, idempotenten Effekt-Executor.
pub trait ActAdapter: Send + Sync {
    fn manifest(&self) -> &AdapterManifest;

    fn execute(&self, invocation: &ActInvocation) -> Result<ActResponse, AdapterCallError>;

    fn execute_fenced(
        &self,
        _invocation: &ActInvocation,
        _fence: &ActFence,
    ) -> Result<ActResponse, AdapterCallError> {
        Err(AdapterCallError::Rejected(
            "endpoint does not support fenced actions".into(),
        ))
    }

    fn close_fence(
        &self,
        _capability: &CapabilityRef,
        _fence: &ActFence,
    ) -> Result<(), AdapterCallError> {
        Err(AdapterCallError::Rejected(
            "endpoint does not support closing reservation fences".into(),
        ))
    }
}
