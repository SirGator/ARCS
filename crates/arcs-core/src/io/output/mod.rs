//! Kontrollierte Ausgabe: gebundener Auftrag, Prüfung, Approval und idempotente Lieferung.

pub use crate::adapters::{OutputAdapter, OutputInvocation, OutputResponse};

mod error;
mod service;
pub use error::OutputError;
pub use service::{
    OutputPreparation, OutputRequest, OutputService, PreparedOutput, TASK_REPORT_SCHEMA,
};

#[cfg(test)]
mod tests;
