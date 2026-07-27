//! Anwendungs-Slices der dauerhaften ARCS-Agentenruntime.

pub mod agent_cycle;
pub mod routing;

// Die bisherige, kompakte Public API bleibt trotz interner Slice-Struktur
// stabil unter `arcs::runtime::*`.
pub use agent_cycle::*;
pub use routing::*;
