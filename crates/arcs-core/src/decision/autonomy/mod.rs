//! Kontrollierte Autonomie nach BIZ-05.
//!
//! Der Dienst entscheidet, ob eine vorgeschlagene Wirkung ohne menschliche
//! Entscheidung wirken darf. Er tut das anhand einer Betreiberregel, die
//! Grenzen für Risiko, Preis, Umgebung und Budget setzt. Ein Vorschlag kann
//! sich keine Rechte heraussuchen: Stufe, Risiko und Preis entstehen aus der
//! konservativen Zusammenführung von Betreiberregel und Vorschlag.
//!
//! Die Freigabe selbst bleibt Teil der bestehenden Kette. Eine autonome
//! Freigabe ist ein System-Approval, das über die bestehende
//! `ApprovalService`-Kette entsteht und damit von Action und Execution
//! unverändert akzeptiert wird.

mod contracts;
mod service;

#[cfg(test)]
mod tests;

pub use contracts::*;
pub use service::{AutonomyRequest, AutonomyService, POLICY_SUBJECT};
