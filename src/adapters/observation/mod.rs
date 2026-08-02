//! Technischer Adaptervertrag für ungefragt eintreffende Beobachtungen.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::registration::CapabilityId;

/// Nicht vertrauenswürdige Nachricht an der Observation-Grenze.
///
/// Schema, interne Identitäten, Herkunft und Trust werden ausschließlich aus
/// Runtime-Konfiguration und Betreiber-Grant abgeleitet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationMessage {
    pub capability_id: CapabilityId,
    pub external_subject: Option<String>,
    pub external_reference: String,
    pub payload: Value,
}
