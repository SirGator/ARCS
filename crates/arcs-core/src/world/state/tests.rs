use super::{StateKey, WorldRevision, WorldState};
use crate::core::SchemaId;
use crate::world::entity::EntityId;

#[test]
fn empty_world_starts_at_revision_zero() {
    let state = WorldState::new();

    assert_eq!(state.revision(), WorldRevision::ZERO);
    assert_eq!(state.entity_count(), 0);
    assert_eq!(state.estimate_count(), 0);
}

#[test]
fn state_keys_have_deterministic_entity_then_schema_order() {
    let mut keys = [
        StateKey::new(
            EntityId::new("entity:b").unwrap(),
            SchemaId("arcs.z.v1".into()),
        ),
        StateKey::new(
            EntityId::new("entity:a").unwrap(),
            SchemaId("arcs.z.v1".into()),
        ),
        StateKey::new(
            EntityId::new("entity:a").unwrap(),
            SchemaId("arcs.a.v1".into()),
        ),
    ];

    keys.sort();

    assert_eq!(keys[0].entity_id().as_str(), "entity:a");
    assert_eq!(keys[0].schema_id().0, "arcs.a.v1");
    assert_eq!(keys[1].schema_id().0, "arcs.z.v1");
    assert_eq!(keys[2].entity_id().as_str(), "entity:b");
}
