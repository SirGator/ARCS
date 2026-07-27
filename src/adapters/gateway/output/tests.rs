use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::adapters::output::{OutputAdapter, OutputResponse};
use crate::adapters::registration::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, CapabilityContract,
    CapabilityDescriptor, CapabilityId,
};
use crate::adapters::{AdapterCallError, ArtifactIdGenerator, GeneratedArtifactIds};
use crate::core::{ActorType, ArtifactId, SchemaRegistry, SourceClass, SubjectId, TrustLevel};
use crate::store::{ArtifactRelation, SqliteArtifactStore};

#[test]
fn invocation_identity_includes_the_result_schema() {
    let capability = CapabilityRef::new("output.chat", "chat.deliver");
    let input = VersionId("candidate-v1".into());

    assert_ne!(
        output_invocation_id(
            &capability,
            &input,
            &SchemaId("arcs.result.first.v1".into()),
        ),
        output_invocation_id(
            &capability,
            &input,
            &SchemaId("arcs.result.second.v1".into()),
        )
    );
}

const RESULT_SCHEMA: &str = r#"{
    "$id": "arcs.result.output_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["delivered"],
    "properties": {
        "delivered": {"type": "boolean"}
    },
    "additionalProperties": false
}"#;

struct FixedClock;

impl crate::adapters::Clock for FixedClock {
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

struct RecordingOutput {
    manifest: AdapterManifest,
    invocations: Arc<Mutex<Vec<OutputInvocation>>>,
    response_reference: String,
    response_payload: serde_json::Value,
    mismatched_response: bool,
}

impl OutputAdapter for RecordingOutput {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn deliver(&self, request: &OutputInvocation) -> Result<OutputResponse, AdapterCallError> {
        self.invocations.lock().unwrap().push(request.clone());
        Ok(OutputResponse {
            invocation_id: if self.mismatched_response {
                "foreign-invocation".into()
            } else {
                request.invocation_id.clone()
            },
            external_reference: self.response_reference.clone(),
            result_payload: self.response_payload.clone(),
        })
    }
}

fn manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("output.chat".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("chat.deliver".into()),
            contract: CapabilityContract::Output {
                accepts: vec![SchemaId("arcs.input.v1".into())],
                emits: vec![SchemaId("arcs.result.output_test.v1".into())],
                idempotent: true,
            },
            required_permissions: vec!["chat.write".into()],
        }],
    }
}

fn grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("output.chat".into()),
        producer_class: ProducerClass::Executor,
        enabled_capabilities: vec![CapabilityId("chat.deliver".into())],
        granted_permissions: vec!["chat.write".into()],
        assigned_trust: TrustLevel::Medium,
        observation_source_kind: None,
        max_payload_bytes: 1024,
        max_external_reference_bytes: 128,
        reasoning_limits: None,
    }
}

fn input() -> Artifact {
    Artifact::new(
        "answer-1",
        "answer-1-v1",
        "input",
        "arcs.input.v1",
        "2026-07-27T11:59:00Z",
        crate::core::Actor {
            actor_type: ActorType::System,
            id: "agent-cycle".into(),
        },
        Source {
            kind: SourceKind::Internal,
            reference: "cycle-1".into(),
        },
        crate::core::Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        "cycle:1",
        json!({"raw_text": "Server prüfen"}),
    )
    .with_subject("current_user_request")
}

fn gateway<'a>(
    schemas: &'a mut SchemaRegistry,
    store: &'a SqliteArtifactStore,
    invocations: Arc<Mutex<Vec<OutputInvocation>>>,
    mismatched_response: bool,
) -> AdapterGateway<'a> {
    let mut gateway = AdapterGateway::new(
        schemas,
        store,
        Box::new(FixedClock),
        Box::new(TestIds { next: 1 }),
    );
    gateway
        .register_output_adapter(
            Box::new(RecordingOutput {
                manifest: manifest(),
                invocations,
                response_reference: "chat-message-42".into(),
                response_payload: json!({"delivered": true}),
                mismatched_response,
            }),
            grant(),
            &[RESULT_SCHEMA],
        )
        .unwrap();
    gateway
}

#[test]
fn delivers_once_and_persists_a_core_owned_result_relation() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = input();
    store.append(&input, &schemas).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut gateway = gateway(&mut schemas, &store, calls.clone(), false);
    let capability = CapabilityRef::new("output.chat", "chat.deliver");
    let result_schema = SchemaId("arcs.result.output_test.v1".into());

    let result = gateway
        .deliver_output(&capability, &input.version_id, &result_schema)
        .unwrap();

    let invocations = calls.lock().unwrap();
    assert_eq!(invocations.len(), 1);
    assert_eq!(invocations[0].capability, capability);
    assert_eq!(invocations[0].artifact_version_id, input.version_id);
    assert_eq!(invocations[0].payload, input.payload);
    assert_eq!(
        invocations[0].subject,
        Some(SubjectId("current_user_request".into()))
    );
    assert_eq!(result.created_by.actor_type, ActorType::Executor);
    assert_eq!(result.created_by.id, "output.chat");
    assert_eq!(result.source.kind, SourceKind::External);
    assert_eq!(result.source.reference, "chat-message-42");
    assert_eq!(result.trust.level, TrustLevel::Medium);
    assert_eq!(result.trust.source_class, SourceClass::External);
    assert_eq!(result.subject, input.subject);
    assert_eq!(
        result.provenance.as_ref().unwrap().parents,
        vec![input.version_id.0.clone()]
    );
    assert_eq!(store.len().unwrap(), 2);
    assert_eq!(
        store.outgoing_relations(&result.version_id).unwrap(),
        vec![ArtifactRelation {
            from: result.version_id.clone(),
            to: input.version_id,
            kind: relation_kinds::result_of(),
        }]
    );
}

#[test]
fn completed_invocation_is_not_delivered_or_persisted_twice() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = input();
    store.append(&input, &schemas).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut gateway = gateway(&mut schemas, &store, calls.clone(), false);
    let capability = CapabilityRef::new("output.chat", "chat.deliver");
    let result_schema = SchemaId("arcs.result.output_test.v1".into());

    gateway
        .deliver_output(&capability, &input.version_id, &result_schema)
        .unwrap();
    let replay = gateway.deliver_output(&capability, &input.version_id, &result_schema);

    assert!(matches!(
        replay,
        Err(AdapterGatewayError::InvocationAlreadyCompleted {
            capability: completed_capability,
            input: completed_input,
            response_schema,
        }) if completed_capability == capability
            && completed_input == input.version_id
            && response_schema == result_schema
    ));
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(store.len().unwrap(), 2);
}

#[test]
fn mismatched_response_cannot_create_a_result_artifact() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = input();
    store.append(&input, &schemas).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut gateway = gateway(&mut schemas, &store, calls.clone(), true);
    let capability = CapabilityRef::new("output.chat", "chat.deliver");
    let result_schema = SchemaId("arcs.result.output_test.v1".into());

    let result = gateway.deliver_output(&capability, &input.version_id, &result_schema);

    assert!(matches!(
        result,
        Err(AdapterGatewayError::InvocationResponseMismatch)
    ));
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(store.len().unwrap(), 1);
}
