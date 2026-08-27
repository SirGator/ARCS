use serde_json::{Value, json};

use super::{ReduceError, Reduction, WorldReducer};
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry,
    CapabilityContract, CapabilityDescriptor, CapabilityId, ProducerClass,
};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds,
    SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, SubjectId, Trust, TrustLevel,
    VersionId,
};
use crate::store::SqliteArtifactStore;
use crate::world::belief::EstimateConfidence;
use crate::world::entity::EntityError;
use crate::world::observation::{
    ObservationLog, ObservationMessage, ObservationService, RecordedObservation,
};
use crate::world::state::{WorldRevision, WorldState};

const SENSOR_SCHEMA_ID: &str = "arcs.sensor_state.demo.v1";
const SENSOR_SCHEMA: &str = r#"{
    "$id": "arcs.sensor_state.demo.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["reading"],
    "properties": {
        "reading": {"type": "number"}
    },
    "additionalProperties": false
}"#;
const ADAPTER_ID: &str = "sensor.demo";
const CAPABILITY_ID: &str = "sensor.observe";

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-08-27T12:00:00Z".into()
    }
}

struct TestIds(u64);

impl ArtifactIdGenerator for TestIds {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
        let sequence = self.0;
        self.0 += 1;
        GeneratedArtifactIds {
            artifact_id: ArtifactId(format!("{artifact_type}-{sequence}")),
            version_id: VersionId(format!("{artifact_type}-{sequence}-v1")),
        }
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(SENSOR_SCHEMA).unwrap();
    schemas
}

fn registry(schemas: &SchemaRegistry, trust: TrustLevel) -> AdapterRegistry {
    let manifest = AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId(ADAPTER_ID.into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId(CAPABILITY_ID.into()),
            contract: CapabilityContract::Observe {
                emits: vec![SchemaId(SENSOR_SCHEMA_ID.into())],
            },
            required_permissions: vec![],
        }],
    };
    let grant = AdapterGrant {
        adapter_id: AdapterId(ADAPTER_ID.into()),
        producer_class: ProducerClass::Adapter,
        enabled_capabilities: vec![CapabilityId(CAPABILITY_ID.into())],
        granted_permissions: vec![],
        assigned_trust: trust,
        ingress_source_kind: Some(SourceKind::Sensor),
        max_payload_bytes: 4096,
        max_external_reference_bytes: 128,
        reasoning_limits: None,
    };
    let mut registry = AdapterRegistry::new();
    registry
        .validate_registration(&manifest, &grant, schemas)
        .unwrap();
    registry.insert_validated(manifest, grant);
    registry
}

fn record(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    ids: &mut dyn ArtifactIdGenerator,
    external_subject: &str,
    reading: f64,
) -> RecordedObservation {
    ObservationService::new(registry, schemas, store, ids, &FixedClock)
        .ingest_recorded(
            &AdapterId(ADAPTER_ID.into()),
            ObservationMessage {
                capability_id: CapabilityId(CAPABILITY_ID.into()),
                external_subject: Some(external_subject.into()),
                external_reference: format!("sensor://{external_subject}"),
                payload: json!({"reading": reading}),
            },
        )
        .unwrap()
}

fn synthetic_recorded(
    version: &str,
    sequence: u64,
    subject: Option<&str>,
    schema_id: &str,
    value: Value,
    recorded_at: &str,
) -> RecordedObservation {
    let artifact_type = schema_id
        .strip_prefix("arcs.")
        .and_then(|body| body.split('.').next())
        .unwrap();
    let mut artifact = Artifact::new(
        format!("artifact:{version}"),
        version,
        artifact_type,
        schema_id,
        recorded_at,
        Actor {
            actor_type: ActorType::Adapter,
            id: "sensor.synthetic".into(),
        },
        Source {
            kind: SourceKind::Sensor,
            reference: format!("sensor://{version}"),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::External,
        },
        format!("observe:{version}"),
        value,
    );
    artifact.subject = subject.map(|value| SubjectId(value.into()));
    RecordedObservation::from_artifact_for_test(artifact, sequence)
}

#[test]
fn recorded_observation_builds_entity_estimate_and_world_state() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, TrustLevel::Medium);
    let mut ids = TestIds(1);
    let observation = record(
        &registry,
        &schemas,
        &store,
        &mut ids,
        "sensor-7/temperature",
        21.5,
    );
    let persisted = store
        .get(&observation.artifact().version_id)
        .unwrap()
        .unwrap();
    let mut world = WorldState::new();

    let reduction = WorldReducer::new()
        .reduce(&mut world, &observation)
        .unwrap();

    assert_eq!(
        reduction,
        Reduction::Applied(WorldRevision::new_for_test(1))
    );
    assert_eq!(persisted, observation.artifact().clone());
    assert_eq!(
        store.len().unwrap(),
        1,
        "read model must not duplicate history"
    );
    assert_eq!(world.entity_count(), 1);
    assert_eq!(world.estimate_count(), 1);
    let subject = observation.artifact().subject.as_ref().unwrap();
    let entity_id = world.entities().next().unwrap().0;
    let entity = world.entity(entity_id).unwrap();
    let estimate = world
        .estimate(entity_id, &SchemaId(SENSOR_SCHEMA_ID.into()))
        .unwrap();
    assert_eq!(entity.id().as_str(), subject.0);
    assert_eq!(entity.canonical_subject(), subject);
    assert_eq!(entity.introduced_by(), &observation.artifact().version_id);
    assert_eq!(estimate.value(), &json!({"reading": 21.5}));
    assert_eq!(estimate.confidence(), &EstimateConfidence::Unknown);
    assert_eq!(estimate.evidence_version(), &persisted.version_id);
    assert_eq!(estimate.evidence_trust(), &persisted.trust);
    assert_eq!(estimate.recorded_at(), persisted.created_at);
}

#[test]
fn later_observation_replaces_only_its_existing_slot() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, TrustLevel::Medium);
    let mut ids = TestIds(1);
    let first = record(
        &registry,
        &schemas,
        &store,
        &mut ids,
        "sensor-7/temperature",
        21.5,
    );
    let second = record(
        &registry,
        &schemas,
        &store,
        &mut ids,
        "sensor-7/temperature",
        22.0,
    );
    let mut world = WorldState::new();
    let reducer = WorldReducer::new();
    reducer.reduce(&mut world, &first).unwrap();
    let entity_id = world.entities().next().unwrap().0.clone();
    let introduced_by = world.entity(&entity_id).unwrap().introduced_by().clone();

    let reduction = reducer.reduce(&mut world, &second).unwrap();

    assert_eq!(reduction.revision().get(), 2);
    assert_eq!(world.entity_count(), 1);
    assert_eq!(world.estimate_count(), 1);
    assert_eq!(
        world.entity(&entity_id).unwrap().introduced_by(),
        &introduced_by
    );
    let estimate = world
        .estimate(&entity_id, &SchemaId(SENSOR_SCHEMA_ID.into()))
        .unwrap();
    assert_eq!(estimate.value(), &json!({"reading": 22.0}));
    assert_eq!(estimate.evidence_version(), &second.artifact().version_id);
    assert_eq!(estimate.confidence(), &EstimateConfidence::Unknown);
    let subject = second.artifact().subject.as_ref().unwrap();
    assert_eq!(
        store
            .history(subject, &SchemaId(SENSOR_SCHEMA_ID.into()))
            .unwrap(),
        vec![first.into_artifact(), second.into_artifact()]
    );
}

#[test]
fn unrelated_entities_are_preserved_in_stable_order() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, TrustLevel::Medium);
    let mut ids = TestIds(1);
    let second = record(&registry, &schemas, &store, &mut ids, "sensor-b", 2.0);
    let first = record(&registry, &schemas, &store, &mut ids, "sensor-a", 1.0);
    let mut world = WorldState::new();
    let reducer = WorldReducer::new();

    reducer.reduce(&mut world, &second).unwrap();
    reducer.reduce(&mut world, &first).unwrap();

    assert_eq!(world.entity_count(), 2);
    assert_eq!(world.estimate_count(), 2);
    let ids = world
        .entities()
        .map(|(id, _)| id.as_str())
        .collect::<Vec<_>>();
    assert!(ids[0] < ids[1]);
}

#[test]
fn different_state_schemas_of_one_entity_use_distinct_slots() {
    let temperature = synthetic_recorded(
        "temperature-v1",
        1,
        Some("entity:sensor-7"),
        "arcs.temperature.demo.v1",
        json!({"celsius": 21.5}),
        "2026-08-27T12:00:00Z",
    );
    let position = synthetic_recorded(
        "position-v1",
        2,
        Some("entity:sensor-7"),
        "arcs.position.demo.v1",
        json!({"x": 4, "y": 2}),
        "2026-08-27T12:00:01Z",
    );
    let mut world = WorldState::new();
    let reducer = WorldReducer::new();

    reducer.reduce(&mut world, &temperature).unwrap();
    reducer.reduce(&mut world, &position).unwrap();

    let entity_id = world.entities().next().unwrap().0;
    assert_eq!(world.entity_count(), 1);
    assert_eq!(world.estimate_count(), 2);
    assert_eq!(
        world
            .estimate(entity_id, &SchemaId("arcs.temperature.demo.v1".into()))
            .unwrap()
            .value(),
        &json!({"celsius": 21.5})
    );
    assert_eq!(
        world
            .estimate(entity_id, &SchemaId("arcs.position.demo.v1".into()))
            .unwrap()
            .value(),
        &json!({"x": 4, "y": 2})
    );
}

#[test]
fn schema_versions_remain_distinct_without_migration_policy() {
    let version_one = synthetic_recorded(
        "temperature-v1",
        1,
        Some("entity:sensor-7"),
        "arcs.temperature.demo.v1",
        json!({"celsius": 21.5}),
        "2026-08-27T12:00:00Z",
    );
    let version_two = synthetic_recorded(
        "temperature-v2",
        2,
        Some("entity:sensor-7"),
        "arcs.temperature.demo.v2",
        json!({"celsius": 21.6, "precision": 0.1}),
        "2026-08-27T12:00:01Z",
    );
    let mut world = WorldState::new();
    let reducer = WorldReducer::new();

    reducer.reduce(&mut world, &version_one).unwrap();
    reducer.reduce(&mut world, &version_two).unwrap();

    let entity_id = world.entities().next().unwrap().0;
    assert_eq!(world.estimate_count(), 2);
    assert!(
        world
            .estimate(entity_id, &SchemaId("arcs.temperature.demo.v1".into()))
            .is_some()
    );
    assert!(
        world
            .estimate(entity_id, &SchemaId("arcs.temperature.demo.v2".into()))
            .is_some()
    );
}

#[test]
fn exact_replay_is_idempotent() {
    let observation = synthetic_recorded(
        "reading-v1",
        1,
        Some("entity:sensor-7"),
        SENSOR_SCHEMA_ID,
        json!({"reading": 21.5}),
        "2026-08-27T12:00:00Z",
    );
    let mut world = WorldState::new();
    let reducer = WorldReducer::new();
    reducer.reduce(&mut world, &observation).unwrap();
    let after_first = world.clone();

    let replay = reducer.reduce(&mut world, &observation).unwrap();

    assert_eq!(replay, Reduction::Unchanged(WorldRevision::new_for_test(1)));
    assert_eq!(world, after_first);
}

#[test]
fn replay_of_older_applied_observation_cannot_rewind_world() {
    let first = synthetic_recorded(
        "reading-v1",
        1,
        Some("entity:sensor-7"),
        SENSOR_SCHEMA_ID,
        json!({"reading": 1.0}),
        "2026-08-27T12:00:00Z",
    );
    let second = synthetic_recorded(
        "reading-v2",
        2,
        Some("entity:sensor-7"),
        SENSOR_SCHEMA_ID,
        json!({"reading": 2.0}),
        "2026-08-27T12:00:01Z",
    );
    let reducer = WorldReducer::new();
    let mut world = WorldState::new();
    reducer.reduce(&mut world, &first).unwrap();
    reducer.reduce(&mut world, &second).unwrap();
    let before_replay = world.clone();

    let replay = reducer.reduce(&mut world, &first);

    assert_eq!(
        replay,
        Err(ReduceError::OutOfOrderObservation {
            last: second.cursor(),
            incoming: first.cursor(),
        })
    );
    assert_eq!(world, before_replay);
}

#[test]
fn reducer_order_not_timestamp_heuristics_determines_latest_estimate() {
    let first = synthetic_recorded(
        "reading-v1",
        1,
        Some("entity:sensor-7"),
        SENSOR_SCHEMA_ID,
        json!({"reading": 1.0}),
        "2026-08-27T12:00:00Z",
    );
    let applied_later = synthetic_recorded(
        "reading-v2",
        2,
        Some("entity:sensor-7"),
        SENSOR_SCHEMA_ID,
        json!({"reading": 2.0}),
        "2025-01-01T00:00:00Z",
    );
    let mut world = WorldState::new();
    let reducer = WorldReducer::new();

    reducer.reduce(&mut world, &first).unwrap();
    reducer.reduce(&mut world, &applied_later).unwrap();

    let (_, estimate) = world.estimates().next().unwrap();
    assert_eq!(estimate.value(), &json!({"reading": 2.0}));
    assert_eq!(estimate.recorded_at(), "2025-01-01T00:00:00Z");
}

#[test]
fn functional_reduction_does_not_mutate_previous_world() {
    let observation = synthetic_recorded(
        "reading-v1",
        1,
        Some("entity:sensor-7"),
        SENSOR_SCHEMA_ID,
        json!({"reading": 21.5}),
        "2026-08-27T12:00:00Z",
    );
    let previous = WorldState::new();

    let (next, reduction) = WorldReducer::new()
        .reduced(&previous, &observation)
        .unwrap();

    assert_eq!(previous.revision(), WorldRevision::ZERO);
    assert_eq!(previous.entity_count(), 0);
    assert_eq!(next.revision().get(), 1);
    assert!(reduction.changed());
}

#[test]
fn errors_leave_entire_world_unchanged() {
    let valid = synthetic_recorded(
        "valid-v1",
        1,
        Some("entity:sensor-7"),
        SENSOR_SCHEMA_ID,
        json!({"reading": 20.0}),
        "2026-08-27T11:59:59Z",
    );
    let invalid = synthetic_recorded(
        "invalid-v1",
        2,
        None,
        SENSOR_SCHEMA_ID,
        json!({"reading": 21.5}),
        "2026-08-27T12:00:00Z",
    );
    let mut world = WorldState::new();
    WorldReducer::new().reduce(&mut world, &valid).unwrap();
    let before = world.clone();

    let result = WorldReducer::new().reduce(&mut world, &invalid);

    assert!(matches!(
        result,
        Err(ReduceError::Entity(EntityError::MissingSubject))
    ));
    assert_eq!(world, before);
}

#[test]
fn revision_overflow_fails_before_mutation() {
    let existing = synthetic_recorded(
        "existing-v1",
        1,
        Some("entity:existing"),
        SENSOR_SCHEMA_ID,
        json!({"reading": 20.0}),
        "2026-08-27T11:59:59Z",
    );
    let observation = synthetic_recorded(
        "reading-v1",
        2,
        Some("entity:sensor-7"),
        SENSOR_SCHEMA_ID,
        json!({"reading": 21.5}),
        "2026-08-27T12:00:00Z",
    );
    let mut world = WorldState::new();
    WorldReducer::new().reduce(&mut world, &existing).unwrap();
    world.set_revision_for_test(WorldRevision::new_for_test(u64::MAX));
    let before = world.clone();

    let result = WorldReducer::new().reduce(&mut world, &observation);

    assert_eq!(result, Err(ReduceError::RevisionOverflow));
    assert_eq!(world, before);
}

#[test]
fn replaying_the_same_ordered_observations_is_deterministic() {
    let observations = [
        synthetic_recorded(
            "a-v1",
            1,
            Some("entity:a"),
            SENSOR_SCHEMA_ID,
            json!({"reading": 1.0}),
            "2026-08-27T12:00:00Z",
        ),
        synthetic_recorded(
            "b-v1",
            2,
            Some("entity:b"),
            SENSOR_SCHEMA_ID,
            json!({"reading": 2.0}),
            "2026-08-27T12:00:01Z",
        ),
        synthetic_recorded(
            "a-v2",
            3,
            Some("entity:a"),
            SENSOR_SCHEMA_ID,
            json!({"reading": 3.0}),
            "2026-08-27T12:00:02Z",
        ),
    ];
    let reducer = WorldReducer::new();
    let mut first_replay = WorldState::new();
    let mut second_replay = WorldState::new();

    for observation in &observations {
        reducer.reduce(&mut first_replay, observation).unwrap();
        reducer.reduce(&mut second_replay, observation).unwrap();
    }

    assert_eq!(first_replay, second_replay);
    assert_eq!(first_replay.revision().get(), 3);
}

#[test]
fn observation_log_rebuilds_world_after_restart_and_skips_other_artifacts() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, TrustLevel::Medium);
    let mut ids = TestIds(1);
    let first = record(&registry, &schemas, &store, &mut ids, "sensor-a", 1.0);
    let unrelated = Artifact::new(
        "input-between",
        "input-between-v1",
        "input",
        "arcs.input.v1",
        "2026-08-27T12:00:00Z",
        Actor {
            actor_type: ActorType::Human,
            id: "operator.test".into(),
        },
        Source {
            kind: SourceKind::Chat,
            reference: "conversation:test".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::Human,
        },
        "input:between",
        json!({"raw_text": "not an observation"}),
    );
    store.append(&unrelated, &schemas).unwrap();
    let second = record(&registry, &schemas, &store, &mut ids, "sensor-b", 2.0);
    let reducer = WorldReducer::new();
    let mut live_world = WorldState::new();
    reducer.reduce(&mut live_world, &first).unwrap();
    reducer.reduce(&mut live_world, &second).unwrap();

    let replayed = ObservationLog::new(&store).after(None).unwrap();
    let mut rebuilt_world = WorldState::new();
    for observation in &replayed {
        reducer.reduce(&mut rebuilt_world, observation).unwrap();
    }

    assert_eq!(replayed.len(), 2);
    assert_eq!(replayed[0].cursor(), first.cursor());
    assert_eq!(replayed[1].cursor(), second.cursor());
    assert_eq!(rebuilt_world, live_world);
    assert_eq!(rebuilt_world.cursor(), Some(second.cursor()));
    assert_eq!(
        ObservationLog::new(&store)
            .after(Some(first.cursor()))
            .unwrap()
            .into_iter()
            .map(|observation| observation.cursor())
            .collect::<Vec<_>>(),
        vec![second.cursor()]
    );
}
