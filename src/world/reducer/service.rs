use crate::world::belief::{EstimateConfidence, StateEstimate};
use crate::world::entity::Entity;
use crate::world::observation::RecordedObservation;
use crate::world::state::{StateKey, WorldRevision, WorldState};

use super::ReduceError;

/// Ergebnis eines World-State-Updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reduction {
    Applied(WorldRevision),
    Unchanged(WorldRevision),
}

impl Reduction {
    pub fn revision(self) -> WorldRevision {
        match self {
            Self::Applied(revision) | Self::Unchanged(revision) => revision,
        }
    }

    pub fn changed(self) -> bool {
        matches!(self, Self::Applied(_))
    }
}

/// Deterministischer Latest-Observation-Reducer für das erste ARCS World Model.
///
/// Für den von einer Observation bestimmten Slot
/// `k(o) = (EntityId(subject(o)), schema_id(o))` gilt exakt:
///
/// `B(b, o)[k(o)] = estimate(o)`
///
/// Alle anderen Slots bleiben unverändert. Die Reihenfolge ist die explizite
/// Reducer-/Commit-Reihenfolge; Zeitstempel werden nicht als versteckte
/// Konfliktheuristik verwendet. Sensorfusion kann später als eigener Belief-
/// Reducer ergänzt werden.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorldReducer;

impl WorldReducer {
    pub fn new() -> Self {
        Self
    }

    /// Aktualisiert `state` transaktional auf Objektebene.
    ///
    /// Alle falliblen Schritte einschließlich Revision-Overflow werden vor
    /// der ersten Mutation ausgeführt. Im Fehlerfall bleibt `state` identisch.
    pub fn reduce(
        &self,
        state: &mut WorldState,
        observation: &RecordedObservation,
    ) -> Result<Reduction, ReduceError> {
        let incoming_cursor = observation.cursor();
        if let Some(last_cursor) = state.cursor()
            && incoming_cursor < last_cursor
        {
            return Err(ReduceError::OutOfOrderObservation {
                last: last_cursor,
                incoming: incoming_cursor,
            });
        }

        let candidate_entity = Entity::from_observation(observation)?;
        let entity = state
            .entity(candidate_entity.id())
            .cloned()
            .unwrap_or(candidate_entity);
        // Der erste Slice besitzt noch kein kalibriertes Beobachtungsmodell.
        // Quantitative Confidence darf deshalb nicht frei vom Caller kommen.
        let estimate =
            StateEstimate::from_observation(&entity, observation, EstimateConfidence::Unknown)?;
        let key = StateKey::new(entity.id().clone(), estimate.schema_id().clone());

        if state.cursor() == Some(incoming_cursor) {
            return if state.current_estimate(&key) == Some(&estimate) {
                Ok(Reduction::Unchanged(state.revision()))
            } else {
                Err(ReduceError::CursorConflict(incoming_cursor))
            };
        }

        let revision = state
            .revision()
            .checked_next()
            .ok_or(ReduceError::RevisionOverflow)?;
        state.commit(revision, incoming_cursor, entity, estimate);
        Ok(Reduction::Applied(revision))
    }

    /// Funktionale Form der Zustandsbeziehung `b_t = B(b_{t-1}, o_t)`.
    ///
    /// Der Live-Pfad kann [`Self::reduce`] ohne vollständigen Clone verwenden;
    /// diese Variante ist für Simulation, Planung und reproduzierbare Tests
    /// nützlich.
    pub fn reduced(
        &self,
        previous: &WorldState,
        observation: &RecordedObservation,
    ) -> Result<(WorldState, Reduction), ReduceError> {
        let mut next = previous.clone();
        let reduction = self.reduce(&mut next, observation)?;
        Ok((next, reduction))
    }
}
