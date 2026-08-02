use serde_json::json;

use super::*;
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry,
    CapabilityContract, CapabilityDescriptor, CapabilityId, ObservationMessage, ProducerClass,
};
use crate::core::{
    ActorType, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds, SchemaId,
    SchemaRegistry, SourceClass, SourceKind, SubjectId, TrustLevel, VersionId,
};
use crate::store::SqliteArtifactStore;

const OBSERVATION_SCHEMA: &str = r#"{
    "$id": "arcs.observation.demo.v2",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["reading"],
    "properties": {
        "reading": {"type": "number"}
    },
    "additionalProperties": false
}"#;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-07-27T12:00:00Z".into()
    }
}

struct TestIds {
    next: u64,
}

impl ArtifactIdGenerator for TestIds {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
        let sequence = self.next;
        self.next += 1;
        GeneratedArtifactIds {
            artifact_id: ArtifactId(format!("{artifact_type}-{sequence}")),
            version_id: VersionId(format!("{artifact_type}-{sequence}-v1")),
        }
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(OBSERVATION_SCHEMA).unwrap();
    schemas
}

fn observe_capability(id: &str, schemas: &[&str]) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId(id.into()),
        contract: CapabilityContract::Observe {
            emits: schemas
                .iter()
                .map(|schema| SchemaId((*schema).into()))
                .collect(),
        },
        required_permissions: vec![],
    }
}

fn observe_manifest(adapter_id: &str) -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId(adapter_id.into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![observe_capability(
            "input.observe",
            &["arcs.observation.demo.v2"],
        )],
    }
}

fn grant(
    adapter_id: &str,
    capabilities: &[&str],
    trust: TrustLevel,
    max_payload_bytes: usize,
) -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId(adapter_id.into()),
        producer_class: ProducerClass::Adapter,
        enabled_capabilities: capabilities
            .iter()
            .map(|capability| CapabilityId((*capability).into()))
            .collect(),
        granted_permissions: vec![],
        assigned_trust: trust,
        observation_source_kind: Some(SourceKind::Sensor),
        max_payload_bytes,
        max_external_reference_bytes: 64,
        reasoning_limits: None,
    }
}

fn registry(
    schemas: &SchemaRegistry,
    manifest: AdapterManifest,
    grant: AdapterGrant,
) -> AdapterRegistry {
    let mut registry = AdapterRegistry::new();
    registry
        .validate_registration(&manifest, &grant, schemas)
        .unwrap();
    registry.insert_validated(manifest, grant);
    registry
}

fn message(payload: serde_json::Value) -> ObservationMessage {
    ObservationMessage {
        capability_id: CapabilityId("input.observe".into()),
        external_subject: Some("sensor-7/temperature".into()),
        external_reference: "sensor://sensor-7".into(),
        payload,
    }
}

#[test]
fn valid_observation_is_stored_with_runtime_envelope() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(
        &schemas,
        observe_manifest("input.demo"),
        grant("input.demo", &["input.observe"], TrustLevel::Medium, 4096),
    );
    let mut ids = TestIds { next: 1 };
    let clock = FixedClock;

    let artifact = ObservationService::new(&registry, &schemas, &store, &mut ids, &clock)
        .ingest(
            &AdapterId("input.demo".into()),
            message(json!({"reading": 21.5})),
        )
        .unwrap();

    assert_eq!(artifact.artifact_id, ArtifactId("observation-1".into()));
    assert_eq!(artifact.version_id, VersionId("observation-1-v1".into()));
    assert_eq!(
        artifact.schema_id,
        SchemaId("arcs.observation.demo.v2".into())
    );
    assert_eq!(artifact.schema_version, 2);
    assert_eq!(artifact.created_at, "2026-07-27T12:00:00Z");
    assert_eq!(artifact.created_by.actor_type, ActorType::Adapter);
    assert_eq!(artifact.created_by.id, "input.demo");
    assert_eq!(artifact.source.kind, SourceKind::Sensor);
    assert_eq!(
        artifact.provenance.as_ref().unwrap().rules_applied,
        vec![
            "observation.capability_authorized",
            "observation.payload_validated",
        ]
    );
    assert_eq!(store.get(&artifact.version_id).unwrap(), Some(artifact));
}

#[test]
fn current_state_is_updated_and_repeated_subject_uses_same_stream() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(
        &schemas,
        observe_manifest("input.demo"),
        grant("input.demo", &["input.observe"], TrustLevel::Medium, 4096),
    );
    let mut ids = TestIds { next: 1 };
    let clock = FixedClock;
    let mut ingress = ObservationService::new(&registry, &schemas, &store, &mut ids, &clock);

    let first = ingress
        .ingest(
            &AdapterId("input.demo".into()),
            message(json!({"reading": 21.5})),
        )
        .unwrap();
    let second = ingress
        .ingest(
            &AdapterId("input.demo".into()),
            message(json!({"reading": 22.0})),
        )
        .unwrap();
    let subject = second.subject.clone().unwrap();

    assert_eq!(first.stream_key, second.stream_key);
    assert_eq!(
        store
            .current(&subject, &SchemaId("arcs.observation.demo.v2".into()))
            .unwrap(),
        Some(second.clone())
    );
    assert_eq!(
        store
            .history(&subject, &SchemaId("arcs.observation.demo.v2".into()))
            .unwrap(),
        vec![first, second]
    );
}

#[test]
fn invalid_payload_does_not_mutate_store() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(
        &schemas,
        observe_manifest("input.demo"),
        grant("input.demo", &["input.observe"], TrustLevel::Medium, 4096),
    );
    let mut ids = TestIds { next: 1 };
    let clock = FixedClock;

    let result = ObservationService::new(&registry, &schemas, &store, &mut ids, &clock)
        .ingest(&AdapterId("input.demo".into()), message(json!({})));

    assert!(matches!(result, Err(ObservationError::InvalidPayload(_))));
    assert!(store.is_empty().unwrap());
}

#[test]
fn unauthorized_capability_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let manifest = AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("input.demo".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![
            observe_capability("input.observe", &["arcs.observation.demo.v2"]),
            observe_capability("input.disabled", &["arcs.observation.demo.v2"]),
        ],
    };
    let registry = registry(
        &schemas,
        manifest,
        grant("input.demo", &["input.observe"], TrustLevel::Medium, 4096),
    );
    let mut ids = TestIds { next: 1 };
    let clock = FixedClock;
    let mut unauthorized = message(json!({"reading": 21.5}));
    unauthorized.capability_id = CapabilityId("input.disabled".into());

    let result = ObservationService::new(&registry, &schemas, &store, &mut ids, &clock)
        .ingest(&AdapterId("input.demo".into()), unauthorized);

    assert!(matches!(
        result,
        Err(ObservationError::Authorization(
            crate::adapters::AdapterRegistryError::CapabilityNotEnabled { .. }
        ))
    ));
    assert!(store.is_empty().unwrap());
}

#[test]
fn act_capability_cannot_push_observation() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let manifest = AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("action.demo".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("device.set".into()),
            contract: CapabilityContract::Act {
                accepts: vec![SchemaId("arcs.input.v1".into())],
                emits: vec![SchemaId("arcs.observation.demo.v2".into())],
                idempotent: true,
            },
            required_permissions: vec!["device.write".into()],
        }],
    };
    let grant = AdapterGrant {
        adapter_id: AdapterId("action.demo".into()),
        producer_class: ProducerClass::Executor,
        enabled_capabilities: vec![CapabilityId("device.set".into())],
        granted_permissions: vec!["device.write".into()],
        assigned_trust: TrustLevel::High,
        observation_source_kind: None,
        max_payload_bytes: 4096,
        max_external_reference_bytes: 64,
        reasoning_limits: None,
    };
    let registry = registry(&schemas, manifest, grant);
    let mut ids = TestIds { next: 1 };
    let clock = FixedClock;
    let mut act_message = message(json!({"reading": 21.5}));
    act_message.capability_id = CapabilityId("device.set".into());

    let result = ObservationService::new(&registry, &schemas, &store, &mut ids, &clock)
        .ingest(&AdapterId("action.demo".into()), act_message);

    assert!(matches!(
        result,
        Err(ObservationError::CapabilityIsNotObserve(_))
    ));
    assert!(store.is_empty().unwrap());
}

#[test]
fn oversized_payload_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(
        &schemas,
        observe_manifest("input.demo"),
        grant("input.demo", &["input.observe"], TrustLevel::Medium, 8),
    );
    let mut ids = TestIds { next: 1 };
    let clock = FixedClock;

    let result = ObservationService::new(&registry, &schemas, &store, &mut ids, &clock).ingest(
        &AdapterId("input.demo".into()),
        message(json!({"reading": 21.5})),
    );

    assert!(matches!(
        result,
        Err(ObservationError::PayloadTooLarge { maximum: 8, .. })
    ));
    assert!(store.is_empty().unwrap());
}

#[test]
fn empty_external_reference_is_rejected_first() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = AdapterRegistry::new();
    let mut ids = TestIds { next: 1 };
    let clock = FixedClock;
    let mut invalid = message(json!({"reading": 21.5}));
    invalid.external_reference = " \t".into();

    let result = ObservationService::new(&registry, &schemas, &store, &mut ids, &clock)
        .ingest(&AdapterId("unknown".into()), invalid);

    assert!(matches!(
        result,
        Err(ObservationError::InvalidExternalReference)
    ));
}

#[test]
fn schema_is_derived_exclusively_from_capability() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(
        &schemas,
        observe_manifest("input.demo"),
        grant("input.demo", &["input.observe"], TrustLevel::Medium, 4096),
    );
    let mut ids = TestIds { next: 1 };
    let clock = FixedClock;

    let artifact = ObservationService::new(&registry, &schemas, &store, &mut ids, &clock)
        .ingest(
            &AdapterId("input.demo".into()),
            message(json!({"reading": 21.5})),
        )
        .unwrap();

    assert_eq!(
        artifact.schema_id,
        SchemaId("arcs.observation.demo.v2".into())
    );
}

#[test]
fn observe_capability_must_emit_exactly_one_schema() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let manifest = AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("input.demo".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![observe_capability(
            "input.observe",
            &["arcs.observation.demo.v2", "arcs.input.v1"],
        )],
    };
    let registry = registry(
        &schemas,
        manifest,
        grant("input.demo", &["input.observe"], TrustLevel::Medium, 4096),
    );
    let mut ids = TestIds { next: 1 };
    let clock = FixedClock;

    let result = ObservationService::new(&registry, &schemas, &store, &mut ids, &clock).ingest(
        &AdapterId("input.demo".into()),
        message(json!({"reading": 21.5})),
    );

    assert!(matches!(
        result,
        Err(ObservationError::InvalidObserveSchemaCount { actual: 2, .. })
    ));
}

#[test]
fn trust_is_taken_from_grant() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(
        &schemas,
        observe_manifest("input.demo"),
        grant("input.demo", &["input.observe"], TrustLevel::High, 4096),
    );
    let mut ids = TestIds { next: 1 };
    let clock = FixedClock;

    let artifact = ObservationService::new(&registry, &schemas, &store, &mut ids, &clock)
        .ingest(
            &AdapterId("input.demo".into()),
            message(json!({"reading": 21.5})),
        )
        .unwrap();

    assert_eq!(artifact.trust.level, TrustLevel::High);
    assert_eq!(artifact.trust.source_class, SourceClass::External);
}

#[test]
fn runtime_namespaces_external_subject() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut registry = registry(
        &schemas,
        observe_manifest("input.demo"),
        grant("input.demo", &["input.observe"], TrustLevel::Medium, 4096),
    );
    let second_manifest = observe_manifest("input.other");
    let second_grant = grant("input.other", &["input.observe"], TrustLevel::Medium, 4096);
    registry
        .validate_registration(&second_manifest, &second_grant, &schemas)
        .unwrap();
    registry.insert_validated(second_manifest, second_grant);
    let mut ids = TestIds { next: 1 };
    let clock = FixedClock;

    let mut ingress = ObservationService::new(&registry, &schemas, &store, &mut ids, &clock);
    let artifact = ingress
        .ingest(
            &AdapterId("input.demo".into()),
            message(json!({"reading": 21.5})),
        )
        .unwrap();
    let other_artifact = ingress
        .ingest(
            &AdapterId("input.other".into()),
            message(json!({"reading": 21.5})),
        )
        .unwrap();

    assert_eq!(
        artifact.subject,
        Some(SubjectId(
            "observe:10:input.demo:13:input.observe:20:sensor-7/temperature".into()
        ))
    );
    assert_ne!(artifact.subject, other_artifact.subject);
    assert_ne!(artifact.stream_key, other_artifact.stream_key);
}

#[test]
fn missing_external_subject_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(
        &schemas,
        observe_manifest("input.demo"),
        grant("input.demo", &["input.observe"], TrustLevel::Medium, 4096),
    );
    let mut ids = TestIds { next: 1 };
    let clock = FixedClock;
    let mut missing = message(json!({"reading": 21.5}));
    missing.external_subject = None;

    let result = ObservationService::new(&registry, &schemas, &store, &mut ids, &clock)
        .ingest(&AdapterId("input.demo".into()), missing);

    assert!(matches!(
        result,
        Err(ObservationError::MissingExternalSubject)
    ));
    assert!(store.is_empty().unwrap());
}
