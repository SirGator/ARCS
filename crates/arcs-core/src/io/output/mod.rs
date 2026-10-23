//! Kontrollierte Ausgabe: gebundener Auftrag, Prüfung, Approval und idempotente Lieferung.

pub use crate::adapters::{OutputAdapter, OutputInvocation, OutputResponse};

mod error;
mod service;
pub use error::OutputError;
pub use service::{OutputRequest, OutputService};

#[cfg(test)]
mod tests;
