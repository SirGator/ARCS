//! Kontrollierte Effektgrenze für freigegebene Candidate-Artefakte.

mod error;
mod service;

pub use error::*;
pub use service::*;

#[cfg(test)]
mod tests;
