//! Vertikaler Slice zum Installieren und Autorisieren externer Adapter.
//!
//! Manifest, Betreiber-Grant und Registry gehören zusammen: Eine
//! Manifestbehauptung wird niemals ohne den getrennten Grant wirksam.

mod catalog;
mod grant;
mod manifest;
mod registry;

pub use catalog::*;
pub use grant::*;

#[cfg(test)]
mod multiplicity_tests;
pub use manifest::*;
pub use registry::*;
