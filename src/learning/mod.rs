//! Explizites lokales Weight Learning auf bereits bestehenden Netzkanten.

mod error;
mod policy;
mod service;

pub use error::*;
pub use policy::*;
pub use service::*;

#[cfg(test)]
mod tests;
