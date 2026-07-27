//! Vertikaler Slice für ungefragt eintreffende Außenwelt-Beobachtungen.
//!
//! Ausschließlich `Observe`-Capabilities verwenden diesen Vertrag. IDs,
//! Source-Klasse, Trust und Zeit bleiben Core-Verantwortung.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::registration::CapabilityId;
use crate::core::{SchemaId, SubjectId};

/// Untrusted Payload an der Eingangsgrenze.
///
/// Adapter dürfen weder IDs noch Actor, Trust, Zeit, Artefakttyp oder
/// Schemaversion liefern. Diese Felder erzeugt der Gateway erst nach
/// Capability- und Schema-Prüfung.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundarySubmission {
    pub capability_id: CapabilityId,
    pub schema_id: SchemaId,
    /// Fachlicher Slot, dessen aktuelle Sicht diese Beobachtung ersetzt.
    pub subject: SubjectId,
    pub external_reference: String,
    pub payload: Value,
}
