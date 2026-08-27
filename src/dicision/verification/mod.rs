//! Auditierbare Prüfung persistierter Artefakte vor jeder späteren Freigabe.

mod contracts;
mod error;
mod service;

pub use contracts::*;
pub use error::*;
pub use service::*;

#[cfg(test)]
mod tests;
