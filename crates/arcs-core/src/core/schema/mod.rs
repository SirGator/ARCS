//! Versionierte JSON-Verträge und ihre Laufzeitprüfung.

pub mod registry;
pub mod types;

pub(crate) use registry::is_rfc3339;
pub use registry::*;
pub use types::*;
