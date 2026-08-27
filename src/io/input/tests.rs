use serde_json::json;

use super::{InputError, InputMessage, InputService};
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry,
    CapabilityContract, CapabilityDescriptor, CapabilityId, ProducerClass,
};
use crate::core::{
    ActorType, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds, SchemaId,
    SchemaRegistry, SourceKind, TrustLevel, VersionId,
};
use crate::store::SqliteArtifactStore;

const ADAPTER_ID: &str = "chat.input-test";
const INPUT_CAPABILITY: &str = "chat.receive";
const INPUT_SCHEMA: &str = "arcs.input.v1";

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-08-08T12:00:00Z".into()
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
    SchemaRegistry::with_bundled_schemas().unwrap()
}

fn input_capability(id: &str, schemas: &[&str]) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId(id.into()),
        contract: CapabilityContract::Input {
            emits: schemas
                .iter()
                .map(|schema| SchemaId((*schema).into()))
                .collect(),
        },
        required_permissions: vec![],
    }
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

fn manifest(capabilities: Vec<CapabilityDescriptor>) -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId(ADAPTER_ID.into()),
        adapter_version: "1.0.0".into(),
        capabilities,
    }
}

fn input_manifest() -> AdapterManifest {
    manifest(vec![input_capability(INPUT_CAPABILITY, &[INPUT_SCHEMA])])
}

fn grant(capabilities: &[&str], max_payload_bytes: usize) -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId(ADAPTER_ID.into()),
        producer_class: ProducerClass::Adapter,
        enabled_capabilities: capabilities
            .iter()
            .map(|capability| CapabilityId((*capability).into()))
            .collect(),
        granted_permissions: vec![],
        assigned_trust: TrustLevel::Medium,
        ingress_source_kind: Some(SourceKind::Chat),
        max_payload_bytes,
        max_external_reference_bytes: 128,
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

fn message(payload: serde_json::Value) -> InputMessage {
    InputMessage {
        capability_id: CapabilityId(INPUT_CAPABILITY.into()),
        external_subject: Some("conversation-7".into()),
        external_reference: "chat://conversation-7/message-1".into(),
        payload,
    }
}

#[test]
fn valid_input_is_stored() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, input_manifest(), grant(&[INPUT_CAPABILITY], 4096));
    let mut ids = TestIds { next: 1 };

    let artifact = InputService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .ingest(
            &AdapterId(ADAPTER_ID.into()),
            message(json!({"raw_text": "open the door"})),
        )
        .unwrap();

    assert_eq!(artifact.artifact_id, ArtifactId("input-1".into()));
    assert_eq!(artifact.version_id, VersionId("input-1-v1".into()));
    assert_eq!(artifact.schema_id, SchemaId(INPUT_SCHEMA.into()));
    assert_eq!(artifact.created_by.actor_type, ActorType::Adapter);
    assert_eq!(artifact.source.kind, SourceKind::Chat);
    assert_eq!(
        artifact.provenance.as_ref().unwrap().rules_applied,
        vec!["input.capability_authorized", "input.payload_validated",]
    );
    assert_eq!(store.get(&artifact.version_id).unwrap(), Some(artifact));
}

#[test]
fn replayed_input_is_not_stored_twice() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, input_manifest(), grant(&[INPUT_CAPABILITY], 4096));
    let mut ids = TestIds { next: 1 };
    let input = message(json!({"raw_text": "open the door"}));
    let mut service = InputService::new(&registry, &schemas, &store, &mut ids, &FixedClock);

    let first = service
        .ingest(&AdapterId(ADAPTER_ID.into()), input.clone())
        .unwrap();
    let replayed = service
        .ingest(&AdapterId(ADAPTER_ID.into()), input)
        .unwrap();

    assert_eq!(replayed, first);
    assert_eq!(store.len().unwrap(), 1);
}

#[test]
fn reused_external_reference_with_changed_payload_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, input_manifest(), grant(&[INPUT_CAPABILITY], 4096));
    let mut ids = TestIds { next: 1 };
    let mut service = InputService::new(&registry, &schemas, &store, &mut ids, &FixedClock);

    service
        .ingest(
            &AdapterId(ADAPTER_ID.into()),
            message(json!({"raw_text": "open the door"})),
        )
        .unwrap();
    let result = service.ingest(
        &AdapterId(ADAPTER_ID.into()),
        message(json!({"raw_text": "delete the file"})),
    );

    assert!(matches!(result, Err(InputError::IdentityConflict(_))));
    assert_eq!(store.len().unwrap(), 1);
}

#[test]
fn input_does_not_create_current_state() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, input_manifest(), grant(&[INPUT_CAPABILITY], 4096));
    let mut ids = TestIds { next: 1 };

    let artifact = InputService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .ingest(
            &AdapterId(ADAPTER_ID.into()),
            message(json!({"raw_text": "where is room 20?"})),
        )
        .unwrap();
    let subject = artifact.subject.clone().unwrap();

    assert_eq!(
        store.get(&artifact.version_id).unwrap(),
        Some(artifact.clone())
    );
    assert_eq!(
        store.current(&subject, &artifact.schema_id).unwrap(),
        None,
        "historical input must not replace a current-state pointer"
    );
}

#[test]
fn input_capability_must_emit_exactly_one_schema() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(
        &schemas,
        manifest(vec![input_capability(
            INPUT_CAPABILITY,
            &[INPUT_SCHEMA, "arcs.reasoning_request.v1"],
        )]),
        grant(&[INPUT_CAPABILITY], 4096),
    );
    let mut ids = TestIds { next: 1 };

    let result = InputService::new(&registry, &schemas, &store, &mut ids, &FixedClock).ingest(
        &AdapterId(ADAPTER_ID.into()),
        message(json!({"raw_text": "open the door"})),
    );

    assert!(matches!(
        result,
        Err(InputError::InvalidInputSchemaCount { actual: 2, .. })
    ));
    assert!(store.is_empty().unwrap());
}

#[test]
fn observe_capability_cannot_push_input() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(
        &schemas,
        manifest(vec![observe_capability(INPUT_CAPABILITY, &[INPUT_SCHEMA])]),
        grant(&[INPUT_CAPABILITY], 4096),
    );
    let mut ids = TestIds { next: 1 };

    let result = InputService::new(&registry, &schemas, &store, &mut ids, &FixedClock).ingest(
        &AdapterId(ADAPTER_ID.into()),
        message(json!({"raw_text": "open the door"})),
    );

    assert!(matches!(result, Err(InputError::CapabilityIsNotInput(_))));
    assert!(store.is_empty().unwrap());
}

#[test]
fn unauthorized_capability_is_rejected() {
    const DISABLED_CAPABILITY: &str = "chat.receive-disabled";

    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(
        &schemas,
        manifest(vec![
            input_capability(INPUT_CAPABILITY, &[INPUT_SCHEMA]),
            input_capability(DISABLED_CAPABILITY, &[INPUT_SCHEMA]),
        ]),
        grant(&[INPUT_CAPABILITY], 4096),
    );
    let mut ids = TestIds { next: 1 };
    let mut unauthorized = message(json!({"raw_text": "open the door"}));
    unauthorized.capability_id = CapabilityId(DISABLED_CAPABILITY.into());

    let result = InputService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .ingest(&AdapterId(ADAPTER_ID.into()), unauthorized);

    assert!(matches!(
        result,
        Err(InputError::Authorization(
            crate::adapters::AdapterRegistryError::CapabilityNotEnabled { .. }
        ))
    ));
    assert!(store.is_empty().unwrap());
}

#[test]
fn invalid_payload_does_not_mutate_store() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, input_manifest(), grant(&[INPUT_CAPABILITY], 4096));
    let mut ids = TestIds { next: 1 };

    let result = InputService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .ingest(&AdapterId(ADAPTER_ID.into()), message(json!({})));

    assert!(matches!(result, Err(InputError::InvalidPayload(_))));
    assert!(store.is_empty().unwrap());
}

#[test]
fn oversized_payload_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, input_manifest(), grant(&[INPUT_CAPABILITY], 8));
    let mut ids = TestIds { next: 1 };

    let result = InputService::new(&registry, &schemas, &store, &mut ids, &FixedClock).ingest(
        &AdapterId(ADAPTER_ID.into()),
        message(json!({"raw_text": "open the door"})),
    );

    assert!(matches!(
        result,
        Err(InputError::PayloadTooLarge { maximum: 8, .. })
    ));
    assert!(store.is_empty().unwrap());
}

#[test]
fn missing_external_subject_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, input_manifest(), grant(&[INPUT_CAPABILITY], 4096));
    let mut ids = TestIds { next: 1 };
    let mut without_subject = message(json!({"raw_text": "open the door"}));
    without_subject.external_subject = None;

    let result = InputService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .ingest(&AdapterId(ADAPTER_ID.into()), without_subject);

    assert!(matches!(result, Err(InputError::MissingExternalSubject)));
    assert!(store.is_empty().unwrap());
}
