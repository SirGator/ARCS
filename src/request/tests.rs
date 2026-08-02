use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry,
    CapabilityContract, CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds,
    SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, SubjectId, Trust, TrustLevel,
    VersionId,
};
use crate::store::SqliteArtifactStore;

const REQUEST_SCHEMA: &str = r#"{
    "$id": "arcs.request.slice_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["metric"],
    "properties": {"metric": {"type": "string", "minLength": 1}},
    "additionalProperties": false
}"#;

const RESPONSE_SCHEMA: &str = r#"{
    "$id": "arcs.observation.request_slice_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["value"],
    "properties": {"value": {"type": "number"}},
    "additionalProperties": false
}"#;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-08-01T12:00:00Z".into()
    }
}

struct TestIds;

impl ArtifactIdGenerator for TestIds {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
        GeneratedArtifactIds {
            artifact_id: ArtifactId(format!("{artifact_type}-response")),
            version_id: VersionId(format!("{artifact_type}-response-v1")),
        }
    }
}

struct RecordingEndpoint {
    manifest: AdapterManifest,
    calls: Arc<Mutex<Vec<RequestInvocation>>>,
    mismatched_correlation: bool,
}

impl RequestAdapter for RecordingEndpoint {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn fetch(
        &self,
        request: &RequestInvocation,
    ) -> Result<RequestResponse, crate::adapters::AdapterCallError> {
        self.calls.lock().unwrap().push(request.clone());
        Ok(RequestResponse {
            invocation_id: if self.mismatched_correlation {
                "foreign-invocation".into()
            } else {
                request.invocation_id.clone()
            },
            external_reference: "https://metrics.example/server-01/cpu".into(),
            payload: json!({"value": 0.92}),
        })
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(REQUEST_SCHEMA).unwrap();
    schemas.register_json(RESPONSE_SCHEMA).unwrap();
    schemas
}

fn manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("metrics.request-test".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("metrics.fetch".into()),
            contract: CapabilityContract::Request {
                accepts: vec![SchemaId("arcs.request.slice_test.v1".into())],
                emits: vec![SchemaId("arcs.observation.request_slice_test.v1".into())],
            },
            required_permissions: vec!["metrics.read".into()],
        }],
    }
}

fn grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("metrics.request-test".into()),
        producer_class: ProducerClass::Adapter,
        enabled_capabilities: vec![CapabilityId("metrics.fetch".into())],
        granted_permissions: vec!["metrics.read".into()],
        assigned_trust: TrustLevel::Medium,
        observation_source_kind: Some(SourceKind::Api),
        max_payload_bytes: 1024,
        max_external_reference_bytes: 256,
        reasoning_limits: None,
    }
}

fn input() -> Artifact {
    Artifact::new(
        "request-1",
        "request-1-v1",
        "request",
        "arcs.request.slice_test.v1",
        "2026-08-01T11:59:00Z",
        Actor {
            actor_type: ActorType::System,
            id: "arcs.runtime".into(),
        },
        Source {
            kind: SourceKind::Internal,
            reference: "cycle-1".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        "cycle:1",
        json!({"metric": "cpu_usage"}),
    )
    .with_subject("server-01/cpu")
}

#[test]
fn response_is_correlated_persisted_and_replay_protected_inside_request_slice() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = input();
    store.append(&input, &schemas).unwrap();
    let endpoint_manifest = manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(endpoint_manifest.clone(), grant(), &schemas, &store)
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let endpoint = RecordingEndpoint {
        manifest: endpoint_manifest,
        calls: Arc::clone(&calls),
        mismatched_correlation: false,
    };
    let mut ids = TestIds;
    let mut service = RequestService::new(&registry, &schemas, &store, &mut ids, &FixedClock);
    let capability = CapabilityRef::new("metrics.request-test", "metrics.fetch");
    let response_schema = SchemaId("arcs.observation.request_slice_test.v1".into());

    let response = service
        .execute(&endpoint, &capability, &input.version_id, &response_schema)
        .unwrap();
    let replay = service.execute(&endpoint, &capability, &input.version_id, &response_schema);

    assert!(matches!(
        replay,
        Err(RequestError::InvocationAlreadyCompleted { .. })
    ));
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(response.subject, Some(SubjectId("server-01/cpu".into())));
    assert_eq!(response.trust.level, TrustLevel::Medium);
    assert_eq!(store.len().unwrap(), 2);
}

#[test]
fn mismatched_external_correlation_does_not_mutate_store() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = input();
    store.append(&input, &schemas).unwrap();
    let endpoint_manifest = manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(endpoint_manifest.clone(), grant(), &schemas, &store)
        .unwrap();
    let endpoint = RecordingEndpoint {
        manifest: endpoint_manifest,
        calls: Arc::new(Mutex::new(Vec::new())),
        mismatched_correlation: true,
    };
    let mut ids = TestIds;
    let mut service = RequestService::new(&registry, &schemas, &store, &mut ids, &FixedClock);

    let result = service.execute(
        &endpoint,
        &CapabilityRef::new("metrics.request-test", "metrics.fetch"),
        &input.version_id,
        &SchemaId("arcs.observation.request_slice_test.v1".into()),
    );

    assert!(matches!(
        result,
        Err(RequestError::InvocationResponseMismatch)
    ));
    assert_eq!(store.len().unwrap(), 1);
}
