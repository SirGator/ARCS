//! Vertikaler Slice für den begrenzten Reasoning-Fallback.
//!
//! Hier leben der externe Port und seine Wireverträge. Die Gateway-Use-Cases
//! zum Aufrufen und Committen geprüfter Kandidaten werden ebenfalls in diesem
//! Slice implementiert.

mod contracts;

pub use contracts::*;
