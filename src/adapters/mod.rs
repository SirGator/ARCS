//! Universelle, domänenneutrale Grenze zu externen ARCS-Adaptern.
//!
//! Konkrete Smart-Home-, Software-, Robotik- oder LLM-Adapter implementieren
//! diese Verträge außerhalb des Cores. Der Core vertraut weder Manifesten noch
//! Payloads ungeprüft und erzeugt alle autoritätsrelevanten Envelope-Felder
//! selbst.

pub mod data;
pub mod gateway;
pub mod observation;
pub mod output;
pub mod port;
pub mod reasoning;
pub mod registration;

pub use data::*;
pub use gateway::*;
pub use observation::*;
pub use output::*;
pub use port::*;
pub use reasoning::*;
pub use registration::*;
