use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterCallError, AdapterGrant, AdapterId, AdapterManifest,
    ArtifactIdGenerator, CapabilityContract, CapabilityDescriptor, CapabilityId, DataAdapter,
    DataResponse, GeneratedArtifactIds,
};
use crate::core::{
    ActorType, ArtifactId, SchemaRegistry, SourceClass, SourceKind, SubjectId, TrustLevel,
};
use crate::store::{ArtifactRelation, SqliteArtifactStore};

#[test]
fn invocation_identity_includes_the_requested_response_schema() {
    let capability = CapabilityRef::new("metrics.gateway-test", "metrics.fetch");
    let request = VersionId("request-v1".into());

    assert_ne!(
        data_invocation_id(
            &capability,
            &request,
            &SchemaId("arcs.observation.first.v1".into()),
        ),
        data_invocation_id(
            &capability,
            &request,
            &SchemaId("arcs.observation.second.v1".into()),
        )
    );
}

const DATA_REQUEST_SCHEMA: &str = r#"{
    "$id": "arcs.data_request.gateway_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["metric"],
    "properties": {
        "metric": {"type": "string", "minLength": 1}
    },
    "additionalProperties": false
}"#;

const OBSERVATION_SCHEMA: &str = r#"{
    "$id": "arcs.observation.gateway_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["value"],
    "properties": {
        "value": {"type": "number"}
    },
    "additionalProperties": false
}"#;

struct FixedClock;

impl crate::adapters::Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-07-27T12:00:00Z".into()
    }
}

struct TestIds;

impl ArtifactIdGenerator for TestIds {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
        GeneratedArtifactIds {
            artifact_id: ArtifactId(format!("{artifact_type}-fetched")),
            version_id: VersionId(format!("{artifact_type}-fetched-v1")),
        }
    }
}

struct RecordingDataAdapter {
    manifest: AdapterManifest,
    invocations: Arc<Mutex<Vec<DataInvocation>>>,
    response_payload: serde_json::Value,
    mismatched_invocation: bool,
}

impl DataAdapter for RecordingDataAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn fetch(&self, invocation: &DataInvocation) -> Result<DataResponse, AdapterCallError> {
        self.invocations.lock().unwrap().push(invocation.clone());
        Ok(DataResponse {
            invocation_id: if self.mismatched_invocation {
                "foreign-invocation".into()
            } else {
                invocation.invocation_id.clone()
            },
            external_reference: "https://metrics.example/server-01/cpu".into(),
            payload: self.response_payload.clone(),
        })
    }
}

fn manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("metrics.gateway-test".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("metrics.fetch".into()),
            contract: CapabilityContract::Data {
                accepts: vec![SchemaId("arcs.data_request.gateway_test.v1".into())],
                emits: vec![SchemaId("arcs.observation.gateway_test.v1".into())],
            },
            required_permissions: vec!["metrics.read".into()],
        }],
    }
}

fn grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("metrics.gateway-test".into()),
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

fn request() -> Artifact {
    Artifact::new(
        "cpu-request",
        "cpu-request-v1",
        "data_request",
        "arcs.data_request.gateway_test.v1",
        "2026-07-27T11:59:59Z",
        crate::core::Actor {
            actor_type: ActorType::System,
            id: "agent-cycle".into(),
        },
        Source {
            kind: SourceKind::Internal,
            reference: "cycle-7".into(),
        },
        crate::core::Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        "cycle:7",
        json!({"metric": "cpu_usage"}),
    )
    .with_subject("server-01/cpu")
}

fn gateway<'a>(
    schemas: &'a mut SchemaRegistry,
    store: &'a SqliteArtifactStore,
) -> AdapterGateway<'a> {
    AdapterGateway::new(schemas, store, Box::new(FixedClock), Box::new(TestIds))
}

#[test]
fn correlated_data_response_is_committed_as_current_with_relations_once() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(DATA_REQUEST_SCHEMA).unwrap();
    schemas.register_json(OBSERVATION_SCHEMA).unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let request = request();
    store.append(&request, &schemas).unwrap();
    let invocations = Arc::new(Mutex::new(Vec::new()));
    let capability = CapabilityRef::new("metrics.gateway-test", "metrics.fetch");

    let observation = {
        let mut gateway = gateway(&mut schemas, &store);
        gateway
            .register_data_adapter(
                Box::new(RecordingDataAdapter {
                    manifest: manifest(),
                    invocations: Arc::clone(&invocations),
                    response_payload: json!({"value": 0.92}),
                    mismatched_invocation: false,
                }),
                grant(),
                &[],
            )
            .unwrap();

        let observation = gateway
            .request_data(
                &capability,
                &request.version_id,
                &SchemaId("arcs.observation.gateway_test.v1".into()),
            )
            .unwrap();

        let replay = gateway.request_data(
            &capability,
            &request.version_id,
            &SchemaId("arcs.observation.gateway_test.v1".into()),
        );
        assert!(matches!(
            replay,
            Err(AdapterGatewayError::InvocationAlreadyCompleted {
                capability: replay_capability,
                input,
                response_schema,
            }) if replay_capability == capability
                && input == request.version_id
                && response_schema == SchemaId("arcs.observation.gateway_test.v1".into())
        ));
        observation
    };

    let recorded = invocations.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].capability, capability);
    assert_eq!(recorded[0].request_version_id, request.version_id);
    assert_eq!(recorded[0].request_payload, request.payload);
    assert_eq!(recorded[0].subject, SubjectId("server-01/cpu".into()));
    assert_eq!(recorded[0].max_response_bytes, 1024);

    assert_eq!(observation.created_by.actor_type, ActorType::Adapter);
    assert_eq!(observation.created_by.id, "metrics.gateway-test");
    assert_eq!(observation.source.kind, SourceKind::Api);
    assert_eq!(observation.trust.level, TrustLevel::Medium);
    assert_eq!(observation.trust.source_class, SourceClass::External);
    assert_eq!(observation.subject, Some(SubjectId("server-01/cpu".into())));
    assert_eq!(
        observation.provenance.as_ref().unwrap().parents,
        vec![request.version_id.0.clone()]
    );
    assert_eq!(
        store
            .current(
                &SubjectId("server-01/cpu".into()),
                &SchemaId("arcs.observation.gateway_test.v1".into()),
            )
            .unwrap(),
        Some(observation.clone())
    );
    assert_eq!(
        store.outgoing_relations(&observation.version_id).unwrap(),
        vec![
            ArtifactRelation {
                from: observation.version_id.clone(),
                to: request.version_id.clone(),
                kind: relation_kinds::fulfills(),
            },
            ArtifactRelation {
                from: observation.version_id.clone(),
                to: request.version_id,
                kind: relation_kinds::caused_by(),
            },
        ]
    );
    assert_eq!(store.len().unwrap(), 2);
}

#[test]
fn mismatched_response_correlation_never_mutates_the_store() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(DATA_REQUEST_SCHEMA).unwrap();
    schemas.register_json(OBSERVATION_SCHEMA).unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let request = request();
    store.append(&request, &schemas).unwrap();
    let invocations = Arc::new(Mutex::new(Vec::new()));
    let mut gateway = gateway(&mut schemas, &store);
    gateway
        .register_data_adapter(
            Box::new(RecordingDataAdapter {
                manifest: manifest(),
                invocations: Arc::clone(&invocations),
                response_payload: json!({"value": 0.92}),
                mismatched_invocation: true,
            }),
            grant(),
            &[],
        )
        .unwrap();

    let result = gateway.request_data(
        &CapabilityRef::new("metrics.gateway-test", "metrics.fetch"),
        &request.version_id,
        &SchemaId("arcs.observation.gateway_test.v1".into()),
    );

    assert!(matches!(
        result,
        Err(AdapterGatewayError::InvocationResponseMismatch)
    ));
    let retry = gateway.request_data(
        &CapabilityRef::new("metrics.gateway-test", "metrics.fetch"),
        &request.version_id,
        &SchemaId("arcs.observation.gateway_test.v1".into()),
    );
    assert!(matches!(
        retry,
        Err(AdapterGatewayError::InvocationResponseMismatch)
    ));
    // Erst ein erfolgreicher atomarer Commit sperrt einen Replay. Eine
    // ungültig korrelierte Antwort darf den Request nicht verbrauchen.
    assert_eq!(invocations.lock().unwrap().len(), 2);
    assert_eq!(store.len().unwrap(), 1);
    assert!(
        store
            .current(
                &SubjectId("server-01/cpu".into()),
                &SchemaId("arcs.observation.gateway_test.v1".into()),
            )
            .unwrap()
            .is_none()
    );
}
