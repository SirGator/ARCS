//! Gemeinsame Adapter-Nachrichten ohne Core, Store oder Gerätewissen.
//! Grants und die Autorisierung einer Verbindung gehören dem Host/Core.

pub mod act;
pub mod data;
mod identifiers;
pub mod input;
pub mod manifest;
pub mod observation;
pub mod output;
pub mod reasoning;
pub mod request;
pub mod stdio;

pub use act::*;
pub use data::*;
pub use identifiers::*;
pub use input::*;
pub use manifest::*;
pub use observation::*;
pub use output::*;
pub use reasoning::*;
pub use request::*;
