use std::fmt;

use serde_json::Value;

use crate::core::{SchemaId, Trust, VersionId};
use crate::world::entity::{Entity, EntityId};
use crate::world::observation::RecordedObservation;

/// Endlicher Wert im geschlossenen Einheitsintervall `[0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct UnitInterval(f64);

impl UnitInterval {
    pub fn new(value: f64) -> Result<Self, ProbabilityError> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(ProbabilityError::OutsideUnitInterval(value));
        }
        // `-0.0` ist mathematisch gleich `0.0`; die Normalisierung hält auch
        // Debug-Ausgaben und spätere Fingerprints kanonisch.
        Ok(Self(if value == 0.0 { 0.0 } else { value }))
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

/// Quantitative Sicherheit einer Schätzung.
///
/// `TrustLevel` ist absichtlich kein Zahlenwert: Herkunftsvertrauen und eine
/// kalibrierte Erfolgswahrscheinlichkeit sind verschiedene Größen. Ohne
/// explizites Beobachtungsmodell bleibt die Confidence deshalb `Unknown`.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum EstimateConfidence {
    #[default]
    Unknown,
    Calibrated {
        probability: UnitInterval,
        /// Versionierte Calibration-/Modell-Evidenz, aus der `q` stammt.
        calibration_version: VersionId,
    },
}

/// Aktuellste Schätzung eines durch `(EntityId, SchemaId)` bezeichneten
/// Zustandsaspekts.
#[derive(Debug, Clone, PartialEq)]
pub struct StateEstimate {
    entity_id: EntityId,
    schema_id: SchemaId,
    value: Value,
    confidence: EstimateConfidence,
    evidence_version: VersionId,
    evidence_trust: Trust,
    recorded_at: String,
}

impl StateEstimate {
    pub(crate) fn from_observation(
        entity: &Entity,
        observation: &RecordedObservation,
        confidence: EstimateConfidence,
    ) -> Result<Self, StateEstimateError> {
        let artifact = observation.artifact();
        if artifact.subject.as_ref() != Some(entity.canonical_subject()) {
            return Err(StateEstimateError::EntitySubjectMismatch);
        }
        Ok(Self {
            entity_id: entity.id().clone(),
            schema_id: artifact.schema_id.clone(),
            value: artifact.payload.clone(),
            confidence,
            evidence_version: artifact.version_id.clone(),
            evidence_trust: artifact.trust.clone(),
            recorded_at: artifact.created_at.clone(),
        })
    }

    pub fn entity_id(&self) -> &EntityId {
        &self.entity_id
    }

    pub fn schema_id(&self) -> &SchemaId {
        &self.schema_id
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn confidence(&self) -> &EstimateConfidence {
        &self.confidence
    }

    pub fn evidence_version(&self) -> &VersionId {
        &self.evidence_version
    }

    pub fn evidence_trust(&self) -> &Trust {
        &self.evidence_trust
    }

    pub fn recorded_at(&self) -> &str {
        &self.recorded_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProbabilityError {
    OutsideUnitInterval(f64),
}

impl fmt::Display for ProbabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideUnitInterval(value) => {
                write!(
                    formatter,
                    "probability must be finite and in [0, 1], got {value}"
                )
            }
        }
    }
}

impl std::error::Error for ProbabilityError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateEstimateError {
    EntitySubjectMismatch,
}

impl fmt::Display for StateEstimateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntitySubjectMismatch => {
                write!(formatter, "entity and observation subjects do not match")
            }
        }
    }
}

impl std::error::Error for StateEstimateError {}
