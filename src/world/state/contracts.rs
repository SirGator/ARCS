use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::core::SchemaId;
use crate::world::belief::StateEstimate;
use crate::world::entity::{Entity, EntityId};
use crate::world::observation::ObservationCursor;

/// Eindeutiger Slot eines Zustandsaspekts in der Weltsicht.
///
/// Mehrere Schemas derselben Entity bleiben getrennt. Eine Temperaturmessung
/// kann dadurch beispielsweise keinen Positionszustand überschreiben. Die
/// Schema-Version ist Teil des Keys; `temperature.v1` und `temperature.v2`
/// bleiben getrennt, bis eine explizite Migrations-/Aspect-Policy existiert.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StateKey {
    entity_id: EntityId,
    schema_id: SchemaId,
}

impl StateKey {
    pub fn new(entity_id: EntityId, schema_id: SchemaId) -> Self {
        Self {
            entity_id,
            schema_id,
        }
    }

    pub fn entity_id(&self) -> &EntityId {
        &self.entity_id
    }

    pub fn schema_id(&self) -> &SchemaId {
        &self.schema_id
    }
}

impl PartialOrd for StateKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StateKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.entity_id
            .cmp(&other.entity_id)
            .then_with(|| self.schema_id.0.cmp(&other.schema_id.0))
    }
}

/// Monotone Version des in-memory World-State.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorldRevision(u64);

impl WorldRevision {
    pub const ZERO: Self = Self(0);

    pub fn get(self) -> u64 {
        self.0
    }

    pub(crate) fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(value: u64) -> Self {
        Self(value)
    }
}

/// Interne Belief-Sicht `b_t` auf die Welt.
///
/// Der Artifact-Store bleibt Audit-Trail und unveränderliche Wahrheit. Dieser
/// Zustand enthält nur die jeweils aktuelle Entity und den aktuellen Estimate
/// je `(EntityId, SchemaId)` und dupliziert keine Historie.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldState {
    revision: WorldRevision,
    cursor: Option<ObservationCursor>,
    entities: BTreeMap<EntityId, Entity>,
    estimates: BTreeMap<StateKey, StateEstimate>,
}

impl WorldState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn revision(&self) -> WorldRevision {
        self.revision
    }

    /// Letzte in diesen Zustand eingearbeitete Position des Artifact-Logs.
    pub fn cursor(&self) -> Option<ObservationCursor> {
        self.cursor
    }

    pub fn entity(&self, id: &EntityId) -> Option<&Entity> {
        self.entities.get(id)
    }

    pub fn estimate(&self, entity_id: &EntityId, schema_id: &SchemaId) -> Option<&StateEstimate> {
        self.estimates
            .get(&StateKey::new(entity_id.clone(), schema_id.clone()))
    }

    pub fn entities(&self) -> impl ExactSizeIterator<Item = (&EntityId, &Entity)> {
        self.entities.iter()
    }

    pub fn estimates(&self) -> impl ExactSizeIterator<Item = (&StateKey, &StateEstimate)> {
        self.estimates.iter()
    }

    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn estimate_count(&self) -> usize {
        self.estimates.len()
    }

    pub(crate) fn current_estimate(&self, key: &StateKey) -> Option<&StateEstimate> {
        self.estimates.get(key)
    }

    /// Wendet ein vollständig vorab geprüftes Update ohne weiteren Fehlerpfad
    /// an. Dadurch kann der Reducer bis zur letzten Zeile fail-closed bleiben.
    pub(crate) fn commit(
        &mut self,
        revision: WorldRevision,
        cursor: ObservationCursor,
        entity: Entity,
        estimate: StateEstimate,
    ) {
        let entity_id = entity.id().clone();
        debug_assert_eq!(estimate.entity_id(), &entity_id);
        let key = StateKey::new(entity_id.clone(), estimate.schema_id().clone());
        self.entities.entry(entity_id).or_insert(entity);
        self.estimates.insert(key, estimate);
        self.revision = revision;
        self.cursor = Some(cursor);
    }

    #[cfg(test)]
    pub(crate) fn set_revision_for_test(&mut self, revision: WorldRevision) {
        self.revision = revision;
    }
}
