//! Verträge an der Grenze für ungefragt eintreffende Beobachtungen.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::adapters::registration::CapabilityId;
use crate::core::Artifact;

pub(super) const CAPABILITY_AUTHORIZED_RULE: &str = "observation.capability_authorized";
pub(super) const PAYLOAD_VALIDATED_RULE: &str = "observation.payload_validated";

/// Nicht vertrauenswürdige Nachricht an der Observation-Grenze.
///
/// Schema, interne Identitäten, Herkunft und Trust werden ausschließlich aus
/// Runtime-Konfiguration und Betreiber-Grant abgeleitet.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationMessage {
    pub capability_id: CapabilityId,
    pub external_subject: Option<String>,
    pub external_reference: String,
    pub payload: Value,
}

/// Monotone Position einer Observation im globalen Artifact-Log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObservationCursor(u64);

impl ObservationCursor {
    pub fn get(self) -> u64 {
        self.0
    }

    pub(super) fn from_store_sequence(sequence: u64) -> Self {
        debug_assert!(sequence > 0, "store sequences start at one");
        Self(sequence)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(sequence: u64) -> Self {
        assert!(sequence > 0);
        Self(sequence)
    }
}

/// Erfolgreich validierte und bereits persistierte Observation.
///
/// Der opake Wrapper ist die Typgrenze zwischen beliebigen Artifacts und dem
/// World-Reducer. Er wird erst nach einem erfolgreichen Store-Commit erzeugt;
/// dadurch kann ein Reasoner kein gewöhnliches Artifact als Weltwahrnehmung
/// ausgeben.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedObservation {
    artifact: Artifact,
    cursor: ObservationCursor,
}

impl RecordedObservation {
    pub(super) fn from_committed(artifact: Artifact, sequence: u64) -> Self {
        Self {
            artifact,
            cursor: ObservationCursor::from_store_sequence(sequence),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_artifact_for_test(artifact: Artifact, sequence: u64) -> Self {
        Self {
            artifact,
            cursor: ObservationCursor::new_for_test(sequence),
        }
    }

    pub fn artifact(&self) -> &Artifact {
        &self.artifact
    }

    pub fn cursor(&self) -> ObservationCursor {
        self.cursor
    }

    pub fn into_artifact(self) -> Artifact {
        self.artifact
    }
}
