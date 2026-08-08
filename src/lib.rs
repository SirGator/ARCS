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
/// Explizites Autoritäts-Gate zwischen Verification und späterer Execution.
pub mod approval;
/// Vertrauenswürdige Domänentypen, Schemas und Validierungsregeln.
pub mod core;
/// Kontrollierte Ausführung ausschließlich explizit freigegebener Kandidaten.
pub mod execution;
/// Einmalige, aktive Ereignisse von externen Eingangsgrenzen.
pub mod input;
/// Explizite lokale Gewichtsänderungen im gespeicherten Artifact-Netz.
pub mod learning;
/// Unaufgefordert von der Außenwelt gemeldeter Weltzustand.
pub mod observation;
/// Auditierbare Erfolgsbewertung ausgeführter Wirkungen vor jeder Lernentscheidung.
pub mod outcome;
/// Kuratierter Fallback für neue oder komplexe Situationen.
pub mod reasoning;
/// Gezielt vom Core angeforderte Daten aus externen Quellen.
pub mod request;
/// Orchestrierung zwischen bekanntem Fast Path und kuratiertem Reasoning.
pub mod runtime;
/// Append-only Persistenz für Artefakte und ihre Versionen.
pub mod store;
/// Auditierbare Prüfung von Kandidaten ohne Freigabe- oder Ausführungswirkung.
pub mod verification;
