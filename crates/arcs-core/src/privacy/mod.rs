//! Kontrollierte Informationsflüsse nach außen.
//!
//! Das Modul bündelt zwei Grenzen, die außerhalb der Modelle gelten:
//!
//! - [`BoundaryPolicy`] legt fest, wohin Daten eines Adapters gelangen dürfen.
//! - [`SecretBroker`] hält Geheimnisse außerhalb des Artefaktnetzes und
//!   vermittelt sie über kurzlebige, zweck- und ressourcengebundene Freigaben.
//!
//! Beide Grenzen werden vor dem Versand eines Auftrags geprüft. Ein Modell,
//! ein Dokument oder eine Toolantwort kann deshalb keine neuen Rechte
//! erzeugen: Sie können nur etwas anfordern, was der Betreiber bereits
//! erteilt hat.

mod boundary;
mod secrets;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub use boundary::{BoundaryPolicy, BoundaryViolation, collect_destinations};
pub use secrets::{
    GRANT_SCHEMA, GrantState, SecretBroker, SecretError, SecretGrant, SecretGrantSpec, SecretId,
    SecretUse, SecretVault, collect_secret_references,
};
