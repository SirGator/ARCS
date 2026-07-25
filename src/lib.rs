//! Öffentliche Rust-Bibliothek des Artifact Reasoning and Control System.
//!
//! Der [`core`] definiert die kontrollierten Datenverträge und deren
//! Validierung. [`store`] stellt die unveränderliche Persistenz dieser
//! Artefakte bereit. Reasoning und Adapter sollen ausschließlich auf diesen
//! kontrollierten Grundlagen aufbauen.

/// Vertrauenswürdige Domänentypen, Schemas und Validierungsregeln.
pub mod core;
/// Append-only Persistenz für Artefakte und ihre Versionen.
pub mod store;
