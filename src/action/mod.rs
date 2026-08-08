//! Materialisierung eines freigegebenen Candidates vor jeder externen Wirkung.

mod error;
mod service;

pub use error::*;
pub use service::*;

#[cfg(test)]
mod tests;
