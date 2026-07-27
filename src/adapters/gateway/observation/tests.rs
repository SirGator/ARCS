use serde_json::json;

use super::*;
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, ArtifactIdGenerator,
    CapabilityContract, CapabilityDescriptor, CapabilityId, GeneratedArtifactIds, ProducerClass,
};
use crate::core::{
    ActorType, ArtifactId, SchemaId, SchemaRegistry, SourceClass, SourceKind, SubjectId,
    TrustLevel, VersionId,
};
use crate::store::SqliteArtifactStore;

const OBSERVATION_V2_SCHEMA: &str = r#"{
    "$id": "arcs.observation.demo.v2",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["reading"],
    "properties": {
        "reading": {"type": "number"}
    },
    "additionalProperties": false
}"#;

const ROUTE_SCHEMA: &str = r#"{
    "$id": "arcs.route_candidate.demo.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["summary"],
    "properties": {
        "summary": {"type": "string", "minLength": 1}
    },
    "additionalProperties": false
}"#;

const RESULT_SCHEMA: &str = r#"{
    "$id": "arcs.result.demo.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["ok"],
    "properties": {
        "ok": {"type": "boolean"}
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

fn gateway<'a>(
    schemas: &'a mut SchemaRegistry,
    store: &'a SqliteArtifactStore,
) -> AdapterGateway<'a> {
    AdapterGateway::new(
        schemas,
        store,
        Box::new(FixedClock),
        Box::new(TestIds { next: 1 }),
    )
}

fn observe_manifest(adapter_id: &str, schema_id: &str) -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId(adapter_id.into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("input.observe".into()),
            contract: CapabilityContract::Observe {
                emits: vec![SchemaId(schema_id.into())],
            },
            required_permissions: vec![],
        }],
    }
}

fn action_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("action.demo".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("device.set".into()),
            contract: CapabilityContract::Act {
                accepts: vec![SchemaId("arcs.route_candidate.demo.v1".into())],
                emits: vec![SchemaId("arcs.result.demo.v1".into())],
                idempotent: true,
            },
            required_permissions: vec!["device.write".into()],
        }],
    }
}

fn grant(
    adapter_id: &str,
    producer_class: ProducerClass,
    capabilities: &[&str],
    permissions: &[&str],
) -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId(adapter_id.into()),
        producer_class,
        enabled_capabilities: capabilities
            .iter()
            .map(|id| CapabilityId((*id).into()))
            .collect(),
        granted_permissions: permissions
            .iter()
            .map(|permission| (*permission).into())
            .collect(),
        assigned_trust: TrustLevel::Medium,
        observation_source_kind: capabilities
            .iter()
            .any(|id| id.ends_with(".observe"))
            .then_some(SourceKind::Sensor),
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        reasoning_limits: None,
    }
}

#[test]
fn valid_observation_gets_an_authoritative_core_envelope() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let artifact = {
        let mut gateway = gateway(&mut schemas, &store);
        let session = gateway
            .register_adapter(
                observe_manifest("input.demo", "arcs.observation.demo.v2"),
                grant(
                    "input.demo",
                    ProducerClass::Adapter,
                    &["input.observe"],
                    &[],
                ),
                &[OBSERVATION_V2_SCHEMA],
            )
            .unwrap();

        gateway
            .submit_boundary(
                &session,
                BoundarySubmission {
                    capability_id: CapabilityId("input.observe".into()),
                    schema_id: SchemaId("arcs.observation.demo.v2".into()),
                    subject: SubjectId("sensor-7/temperature".into()),
                    external_reference: "sensor-7".into(),
                    payload: json!({"reading": 21.5}),
                },
            )
            .unwrap()
    };

    assert_eq!(artifact.artifact_id, ArtifactId("observation-1".into()));
    assert_eq!(artifact.version_id, VersionId("observation-1-v1".into()));
    assert_eq!(artifact.version, 1);
    assert_eq!(artifact.artifact_type, "observation");
    assert_eq!(
        artifact.schema_id,
        SchemaId("arcs.observation.demo.v2".into())
    );
    assert_eq!(artifact.schema_version, 2);
    assert_eq!(artifact.created_at, "2026-07-27T12:00:00Z");
    assert_eq!(artifact.created_by.actor_type, ActorType::Adapter);
    assert_eq!(artifact.created_by.id, "input.demo");
    assert_eq!(artifact.source.kind, SourceKind::Sensor);
    assert_eq!(artifact.source.reference, "sensor-7");
    assert_eq!(artifact.trust.level, TrustLevel::Medium);
    assert_eq!(artifact.trust.source_class, SourceClass::External);
    assert_eq!(
        artifact.stream_key,
        "observe:10:input.demo:20:sensor-7/temperature:24:arcs.observation.demo.v2"
    );
    assert_eq!(
        artifact.subject,
        Some(SubjectId("sensor-7/temperature".into()))
    );
    assert_eq!(
        artifact.tags,
        vec!["adapter:input.demo", "capability:input.observe"]
    );
    assert_eq!(artifact.payload, json!({"reading": 21.5}));
    assert_eq!(
        artifact
            .provenance
            .as_ref()
            .and_then(|provenance| provenance.transform.as_deref()),
        Some("adapter:input.demo")
    );
    assert_eq!(store.len().unwrap(), 1);
    assert_eq!(
        store.get(&artifact.version_id).unwrap().as_ref(),
        Some(&artifact)
    );
}

#[test]
fn repeated_observations_share_a_stable_stream_and_remain_queryable() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let schema_id = SchemaId("arcs.observation.demo.v2".into());
    let subject = SubjectId("sensor-7/temperature".into());

    let (first, second) = {
        let mut gateway = gateway(&mut schemas, &store);
        let session = gateway
            .register_adapter(
                observe_manifest("input.demo", &schema_id.0),
                grant(
                    "input.demo",
                    ProducerClass::Adapter,
                    &["input.observe"],
                    &[],
                ),
                &[OBSERVATION_V2_SCHEMA],
            )
            .unwrap();

        let first = gateway
            .submit_boundary(
                &session,
                BoundarySubmission {
                    capability_id: CapabilityId("input.observe".into()),
                    schema_id: schema_id.clone(),
                    subject: subject.clone(),
                    external_reference: "reading-1".into(),
                    payload: json!({"reading": 21.5}),
                },
            )
            .unwrap();
        let second = gateway
            .submit_boundary(
                &session,
                BoundarySubmission {
                    capability_id: CapabilityId("input.observe".into()),
                    schema_id: schema_id.clone(),
                    subject: subject.clone(),
                    external_reference: "reading-2".into(),
                    payload: json!({"reading": 22.0}),
                },
            )
            .unwrap();
        (first, second)
    };

    assert_ne!(first.artifact_id, second.artifact_id);
    assert_eq!(first.stream_key, second.stream_key);
    assert_eq!(
        store.history(&subject, &schema_id).unwrap(),
        vec![first, second.clone()]
    );
    assert_eq!(store.current(&subject, &schema_id).unwrap(), Some(second));
}

#[test]
fn invalid_observation_payload_never_mutates_the_store() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut gateway = gateway(&mut schemas, &store);
    let session = gateway
        .register_adapter(
            observe_manifest("input.demo", "arcs.input.v1"),
            grant(
                "input.demo",
                ProducerClass::Adapter,
                &["input.observe"],
                &[],
            ),
            &[],
        )
        .unwrap();

    let result = gateway.submit_boundary(
        &session,
        BoundarySubmission {
            capability_id: CapabilityId("input.observe".into()),
            schema_id: SchemaId("arcs.input.v1".into()),
            subject: SubjectId("current_user_request".into()),
            external_reference: "request-1".into(),
            payload: json!({}),
        },
    );

    assert!(matches!(
        result,
        Err(AdapterGatewayError::InvalidPayload(_))
    ));
    assert!(store.is_empty().unwrap());
}

#[test]
fn act_capability_cannot_push_an_uncorrelated_result() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut gateway = gateway(&mut schemas, &store);
    let action_session = gateway
        .register_adapter(
            action_manifest(),
            grant(
                "action.demo",
                ProducerClass::Executor,
                &["device.set"],
                &["device.write"],
            ),
            &[ROUTE_SCHEMA, RESULT_SCHEMA],
        )
        .unwrap();

    let result = gateway.submit_boundary(
        &action_session,
        BoundarySubmission {
            capability_id: CapabilityId("device.set".into()),
            schema_id: SchemaId("arcs.result.demo.v1".into()),
            subject: SubjectId("device-1".into()),
            external_reference: "self-declared-success".into(),
            payload: json!({"ok": true}),
        },
    );

    assert!(matches!(
        result,
        Err(AdapterGatewayError::CapabilityCannotPush(_))
    ));
    assert!(store.is_empty().unwrap());
}

#[test]
fn adapter_session_cannot_impersonate_another_adapter() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut gateway = gateway(&mut schemas, &store);
    gateway
        .register_adapter(
            observe_manifest("input.demo", "arcs.input.v1"),
            grant(
                "input.demo",
                ProducerClass::Adapter,
                &["input.observe"],
                &[],
            ),
            &[],
        )
        .unwrap();
    let action_session = gateway
        .register_adapter(
            action_manifest(),
            grant(
                "action.demo",
                ProducerClass::Executor,
                &["device.set"],
                &["device.write"],
            ),
            &[ROUTE_SCHEMA, RESULT_SCHEMA],
        )
        .unwrap();

    let result = gateway.submit_boundary(
        &action_session,
        BoundarySubmission {
            capability_id: CapabilityId("input.observe".into()),
            schema_id: SchemaId("arcs.input.v1".into()),
            subject: SubjectId("current_user_request".into()),
            external_reference: "spoof".into(),
            payload: json!({"raw_text": "spoofed"}),
        },
    );

    assert!(matches!(
        result,
        Err(AdapterGatewayError::AdapterRegistry(
            AdapterRegistryError::CapabilityNotEnabled { .. }
        ))
    ));
    assert!(store.is_empty().unwrap());
}

#[test]
fn adapter_session_is_bound_to_the_gateway_that_issued_it() {
    let mut schemas_a = SchemaRegistry::with_bundled_schemas().unwrap();
    let store_a = SqliteArtifactStore::in_memory().unwrap();
    let mut gateway_a = gateway(&mut schemas_a, &store_a);
    let session_a = gateway_a
        .register_adapter(
            observe_manifest("input.demo", "arcs.input.v1"),
            grant(
                "input.demo",
                ProducerClass::Adapter,
                &["input.observe"],
                &[],
            ),
            &[],
        )
        .unwrap();

    let mut schemas_b = SchemaRegistry::with_bundled_schemas().unwrap();
    let store_b = SqliteArtifactStore::in_memory().unwrap();
    let mut gateway_b = gateway(&mut schemas_b, &store_b);
    let session_b = gateway_b
        .register_adapter(
            observe_manifest("input.demo", "arcs.input.v1"),
            grant(
                "input.demo",
                ProducerClass::Adapter,
                &["input.observe"],
                &[],
            ),
            &[],
        )
        .unwrap();

    // Beide lokalen Tokenräume beginnen bei 1. Allein der Token darf daher
    // niemals zur Authentifizierung an einer anderen Instanz ausreichen.
    assert_eq!(session_a.token, 1);
    assert_eq!(session_b.token, 1);
    assert_ne!(session_a.gateway_instance_id, session_b.gateway_instance_id);

    let result = gateway_b.submit_boundary(
        &session_a,
        BoundarySubmission {
            capability_id: CapabilityId("input.observe".into()),
            schema_id: SchemaId("arcs.input.v1".into()),
            subject: SubjectId("current_user_request".into()),
            external_reference: "cross-gateway-replay".into(),
            payload: json!({"raw_text": "must be rejected"}),
        },
    );

    assert!(matches!(
        result,
        Err(AdapterGatewayError::InvalidAdapterSession)
    ));
    assert!(store_b.is_empty().unwrap());
}

#[test]
fn oversized_external_reference_is_rejected_without_mutation() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut gateway = gateway(&mut schemas, &store);
    let session = gateway
        .register_adapter(
            observe_manifest("input.demo", "arcs.input.v1"),
            grant(
                "input.demo",
                ProducerClass::Adapter,
                &["input.observe"],
                &[],
            ),
            &[],
        )
        .unwrap();

    let result = gateway.submit_boundary(
        &session,
        BoundarySubmission {
            capability_id: CapabilityId("input.observe".into()),
            schema_id: SchemaId("arcs.input.v1".into()),
            subject: SubjectId("current_user_request".into()),
            external_reference: "x".repeat(513),
            payload: json!({"raw_text": "valid"}),
        },
    );

    assert!(matches!(
        result,
        Err(AdapterGatewayError::ExternalReferenceTooLarge {
            actual: 513,
            maximum: 512
        })
    ));
    assert!(store.is_empty().unwrap());
}
