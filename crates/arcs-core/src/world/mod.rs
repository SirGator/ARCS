//! Interne, kontrolliert aufgebaute Sicht auf die Außenwelt.

/// Quantitative oder ausdrücklich unbekannte Sicherheit von State Estimates.
pub mod belief;
/// Kanonische Identitäten beobachteter Dinge.
pub mod entity;
/// Validierte, persistierte Wahrnehmung an einer Adapter-Grenze.
pub mod observation;
/// Explizite Zustandsfunktion `b_t = B(b_{t-1}, o_t)`.
pub mod reducer;
/// Aktuelle, deterministische Belief-Sicht auf Entities und Zustandsaspekte.
pub mod state;

pub use belief::*;
pub use entity::*;
pub use reducer::*;
pub use state::*;
