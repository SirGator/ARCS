//! Universelle, domänenneutrale Grenze zu externen ARCS-Adaptern.
//!
//! Konkrete Smart-Home-, Software-, Robotik- oder LLM-Adapter implementieren
//! diese Verträge außerhalb des Cores. Der Core vertraut weder Manifesten noch
//! Payloads ungeprüft und erzeugt alle autoritätsrelevanten Envelope-Felder
//! selbst.

pub mod observation;
pub mod output;
pub mod port;
pub mod registration;

pub use observation::*;
pub use output::*;
pub use port::*;
pub use registration::*;

// Zeitlich begrenzte Quellkompatibilität. Die Implementierungen und
// Primärtypen leben bereits in ihren jeweiligen Top-Level-Slices.
#[doc(hidden)]
pub use crate::reasoning::*;
#[doc(hidden)]
pub use crate::request::{DataAdapter, DataInvocation, DataResponse};
