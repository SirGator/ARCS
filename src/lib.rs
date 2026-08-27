//! Öffentliche Rust-Bibliothek des Artifact Reasoning and Control System.
//!
//! Der [`core`] definiert die kontrollierten Datenverträge und deren
//! Validierung. [`store`] stellt die unveränderliche Persistenz dieser
//! Artefakte bereit. Reasoning und Adapter sollen ausschließlich auf diesen
//! kontrollierten Grundlagen aufbauen.

/// Materialisierung freigegebener Kandidaten zu kontrollierten Action-Aufträgen.
pub mod action;
/// Universelle Verträge und kontrollierte Grenze für externe Adapter.
pub mod adapters;
/// Vertrauenswürdige Domänentypen, Schemas und Validierungsregeln.
pub mod core;
/// Prüfung und explizite Autorisierung vorgeschlagener Handlungen.
#[path = "dicision/mod.rs"]
pub mod decision;
/// Kontrollierte Kommunikation mit externen Systemen.
pub mod io;
/// Explizite lokale Gewichtsänderungen im gespeicherten Artifact-Netz.
pub mod learning;
/// Kuratierter Fallback für neue oder komplexe Situationen.
pub mod reasoning;
/// Orchestrierung zwischen bekanntem Fast Path und kuratiertem Reasoning.
pub mod runtime;
/// Append-only Persistenz für Artefakte und ihre Versionen.
pub mod store;
/// Interne, kontrolliert aufgebaute Sicht auf die Außenwelt.
pub mod world;

// Rückwärtskompatible Modulpfade für bestehende Nutzer der Bibliothek.
pub use action::{execution, outcome};
pub use decision::{approval, verification};
pub use io::{input, request};
pub use world::observation;
