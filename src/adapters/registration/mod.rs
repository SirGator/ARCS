//! Vertikaler Slice zum Installieren und Autorisieren externer Adapter.
//!
//! Manifest, Betreiber-Grant und Registry gehören zusammen: Eine
//! Manifestbehauptung wird niemals ohne den getrennten Grant wirksam.

mod grant;
mod manifest;
mod registry;

pub use grant::*;
pub use manifest::*;
pub use registry::*;
