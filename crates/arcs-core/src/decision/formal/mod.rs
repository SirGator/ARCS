//! Formale Aussagen, Nachweise und ihre Verbindlichkeit.
//!
//! Der Core definiert die Verträge und die Bewertung. Das formale Werkzeug
//! bleibt extern: Es prüft, der Core entscheidet, ob das Ergebnis als
//! Nachweis gilt.

mod contracts;
mod service;

pub use arcs_adapter_protocol::formal::{
    FormalCheckOutcome, FormalCheckRequest, FormalFinding, FormalFindingVerdict, FormalSource,
    FormalVerdict, Toolchain,
};
pub use contracts::*;
pub use service::*;

#[cfg(test)]
mod tests;
