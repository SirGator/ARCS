use serde_json::json;

use super::{Entity, EntityError, EntityId, MAX_ENTITY_ID_BYTES};
use crate::core::{Actor, ActorType, Artifact, Source, SourceClass, SourceKind, Trust, TrustLevel};
use crate::world::observation::RecordedObservation;

fn recorded(subject: Option<&str>) -> RecordedObservation {
    let mut artifact = Artifact::new(
        "observation-1",
        "observation-1-v1",
        "sensor_state",
        "arcs.sensor_state.demo.v1",
        "2026-08-27T12:00:00Z",
        Actor {
            actor_type: ActorType::Adapter,
            id: "sensor.demo".into(),
        },
        Source {
            kind: SourceKind::Sensor,
            reference: "sensor://demo".into(),
        },
        Trust {
            level: TrustLevel::Medium,
            source_class: SourceClass::External,
        },
        "observe:demo",
        json!({"reading": 21.5}),
    );
    artifact.subject = subject.map(|value| crate::core::SubjectId(value.into()));
    RecordedObservation::from_artifact_for_test(artifact, 1)
}

#[test]
fn entity_id_rejects_invalid_text() {
    for invalid in ["", " \t", "contains\ncontrol"] {
        assert!(matches!(
            EntityId::new(invalid),
            Err(EntityError::InvalidId(_))
        ));
    }
    assert!(matches!(
        EntityId::new("x".repeat(MAX_ENTITY_ID_BYTES + 1)),
        Err(EntityError::InvalidId(_))
    ));
}

#[test]
fn exact_subject_resolution_is_stable_and_injective() {
    let first = Entity::from_observation(&recorded(Some("observe:adapter-a:sensor-7"))).unwrap();
    let repeated = Entity::from_observation(&recorded(Some("observe:adapter-a:sensor-7"))).unwrap();
    let other = Entity::from_observation(&recorded(Some("observe:adapter-b:sensor-7"))).unwrap();

    assert_eq!(first.id(), repeated.id());
    assert_ne!(first.id(), other.id());
    assert_eq!(first.id().as_str(), first.canonical_subject().0);
}

#[test]
fn observation_without_subject_cannot_create_entity() {
    assert_eq!(
        Entity::from_observation(&recorded(None)),
        Err(EntityError::MissingSubject)
    );
}
