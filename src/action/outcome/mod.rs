//! Auditierbare Bewertung einer externen Wirkung ohne automatische Lernwirkung.

mod contracts;
mod error;
mod service;

pub use contracts::*;
pub use error::*;
pub use service::*;

#[cfg(test)]
mod tests;
