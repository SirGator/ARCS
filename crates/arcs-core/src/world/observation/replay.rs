use crate::core::Artifact;
use crate::store::{SqliteArtifactStore, StoreError};

use super::{
    CAPABILITY_AUTHORIZED_RULE, ObservationCursor, PAYLOAD_VALIDATED_RULE, RecordedObservation,
};

/// Geordneter Replay-Zugang zum unveränderlichen Observation-Log.
pub struct ObservationLog<'a> {
    store: &'a SqliteArtifactStore,
}

impl<'a> ObservationLog<'a> {
    pub fn new(store: &'a SqliteArtifactStore) -> Self {
        Self { store }
    }

    /// Liefert alle nach `cursor` committed Observations in stabiler Reihenfolge.
    ///
    /// `None` beginnt am Anfang des Logs und dient dem vollständigen Aufbau
    /// eines leeren World-State nach einem Neustart.
    pub fn after(
        &self,
        cursor: Option<ObservationCursor>,
    ) -> Result<Vec<RecordedObservation>, StoreError> {
        let sequence = cursor.map_or(0, ObservationCursor::get);
        Ok(self
            .store
            .committed_after(sequence)?
            .into_iter()
            .filter_map(|committed| {
                is_recorded_observation(&committed.artifact).then(|| {
                    RecordedObservation::from_committed(committed.artifact, committed.sequence)
                })
            })
            .collect())
    }
}

fn is_recorded_observation(artifact: &Artifact) -> bool {
    let Some(provenance) = &artifact.provenance else {
        return false;
    };
    artifact.subject.is_some()
        && artifact.stream_key.starts_with("observe:")
        && artifact.tags.iter().any(|tag| tag.starts_with("adapter:"))
        && artifact
            .tags
            .iter()
            .any(|tag| tag.starts_with("capability:"))
        && provenance
            .rules_applied
            .iter()
            .any(|rule| rule == CAPABILITY_AUTHORIZED_RULE)
        && provenance
            .rules_applied
            .iter()
            .any(|rule| rule == PAYLOAD_VALIDATED_RULE)
}
