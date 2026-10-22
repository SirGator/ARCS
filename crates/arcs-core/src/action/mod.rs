//! Materialisierung eines freigegebenen Candidates vor jeder externen Wirkung.

pub mod execution;
pub mod outcome;

mod error;
mod service;

pub use error::*;
pub use service::*;

#[cfg(test)]
mod tests;
