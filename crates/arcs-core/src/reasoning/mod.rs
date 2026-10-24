//! Vertikaler Slice für neue und komplexe Situationen.
//!
//! Reasoning erhält ausschließlich explizit ausgewählten Kontext und ein
//! begrenztes Budget. Externe Modelle, Planner oder Solver dürfen Kandidaten
//! vorschlagen, aber weder Trust noch Freigaben oder Ausführungsrechte setzen.
//! Erst der Core validiert und persistiert einen solchen Vorschlag.
//!
//! Der Slice besitzt seine Wire-Verträge selbst. Gemeinsame Capability- und
//! Grant-Typen werden später separat aus dem alten Adapter-Sammelmodul gelöst.

pub mod general;
pub mod routing;
pub use arcs_adapter_protocol::reasoning_v2::*;

mod contracts;
mod error;
mod proposal;
mod service;

pub use contracts::*;
pub use error::ReasoningError;
pub use service::ReasoningService;

#[cfg(test)]
mod tests;
