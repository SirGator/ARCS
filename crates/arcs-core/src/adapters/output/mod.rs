//! Vertikaler Adaptervertrag für korrelierte externe Ausgabe.

use crate::adapters::{AdapterCallError, AdapterManifest};
pub use arcs_adapter_protocol::{OutputInvocation, OutputResponse};

/// Interner Port zu Chat, UI, Datei, API oder einer anderen Ausgabesenke.
pub trait OutputAdapter: Send + Sync {
    fn manifest(&self) -> &AdapterManifest;

    fn deliver(&self, request: &OutputInvocation) -> Result<OutputResponse, AdapterCallError>;
}
