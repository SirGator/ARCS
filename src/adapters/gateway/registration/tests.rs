use crate::adapters::AdapterCallError;
use crate::adapters::gateway::{SequenceIdGenerator, SystemClock};
use crate::adapters::reasoning::{ReasoningInvocation, ReasoningLimits, ReasoningResponse};
use crate::adapters::registration::{
    ADAPTER_PROTOCOL_VERSION, CapabilityContract, CapabilityDescriptor, CapabilityId,
};
use crate::core::{RegistryError, SchemaId, SourceKind, TrustLevel};
use crate::store::SqliteArtifactStore;

use super::*;

const OBSERVATION_SCHEMA: &str = r#"{
    "$id": "arcs.observation.registration.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["reading"],
    "properties": {
        "reading": {"type": "number"}
    },
    "additionalProperties": false
}"#;

const AUTHORITATIVE_ACTION_SCHEMA: &str = r#"{
    "$id": "arcs.action.registration.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["command"],
    "properties": {
        "command": {"type": "string", "minLength": 1}
    },
    "additionalProperties": false
}"#;

fn gateway<'a>(
    schemas: &'a mut SchemaRegistry,
    store: &'a SqliteArtifactStore,
) -> AdapterGateway<'a> {
    AdapterGateway::new(
        schemas,
        store,
        Box::new(SystemClock),
        Box::new(SequenceIdGenerator::new("registration-test")),
    )
}

fn observation_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("sensor.registration".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("sensor.observe".into()),
            contract: CapabilityContract::Observe {
                emits: vec![SchemaId("arcs.observation.registration.v1".into())],
            },
            required_permissions: vec![],
        }],
    }
}

fn observation_grant(adapter_id: &str) -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId(adapter_id.into()),
        producer_class: ProducerClass::Adapter,
        enabled_capabilities: vec![CapabilityId("sensor.observe".into())],
        granted_permissions: vec![],
        assigned_trust: TrustLevel::Medium,
        observation_source_kind: Some(SourceKind::Sensor),
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        reasoning_limits: None,
    }
}

fn reasoning_manifest(schema_id: &str) -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("reasoning.registration".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("reasoning.propose".into()),
            contract: CapabilityContract::Reason {
                emits: vec![SchemaId(schema_id.into())],
            },
            required_permissions: vec![],
        }],
    }
}

fn reasoning_grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("reasoning.registration".into()),
        producer_class: ProducerClass::Model,
        enabled_capabilities: vec![CapabilityId("reasoning.propose".into())],
        granted_permissions: vec![],
        assigned_trust: TrustLevel::Low,
        observation_source_kind: None,
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        reasoning_limits: Some(ReasoningLimits {
            max_context_items: 8,
            max_context_bytes: 8192,
            max_output_tokens: 1024,
            max_output_bytes: 8192,
            max_candidates: 8,
        }),
    }
}

struct NeverCalledReasoner {
    manifest: AdapterManifest,
}

impl ReasoningAdapter for NeverCalledReasoner {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn propose(
        &self,
        _request: &ReasoningInvocation,
    ) -> Result<ReasoningResponse, AdapterCallError> {
        panic!("registration must not invoke the reasoning endpoint")
    }
}

#[test]
fn schema_bundle_registration_is_atomic_when_one_schema_is_invalid() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let invalid_schema = r##"{
        "$id": "arcs.invalid.registration.v1",
        "type": "object",
        "properties": {
            "reading": {"$ref": "#/definitions/reading"}
        }
    }"##;

    {
        let mut gateway = gateway(&mut schemas, &store);
        let result = gateway.register_adapter(
            observation_manifest(),
            observation_grant("sensor.registration"),
            &[OBSERVATION_SCHEMA, invalid_schema],
        );

        assert!(matches!(
            result,
            Err(AdapterGatewayError::SchemaRegistry(
                RegistryError::UnsupportedKeyword { .. }
            ))
        ));
        assert!(
            gateway
                .registry()
                .get(&AdapterId("sensor.registration".into()))
                .is_none()
        );
        assert!(gateway.adapter_sessions.is_empty());
    }

    // Obwohl das erste Dokument gültig war, darf kein Teil des Pakets
    // sichtbar werden, wenn ein späteres Dokument scheitert.
    assert!(
        schemas
            .get(&SchemaId("arcs.observation.registration.v1".into()))
            .is_none()
    );
}

#[test]
fn schema_stage_is_rolled_back_when_operator_grant_is_invalid() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();

    {
        let mut gateway = gateway(&mut schemas, &store);
        let result = gateway.register_adapter(
            observation_manifest(),
            observation_grant("different.adapter"),
            &[OBSERVATION_SCHEMA],
        );

        assert!(matches!(
            result,
            Err(AdapterGatewayError::AdapterRegistry(
                crate::adapters::registration::AdapterRegistryError::GrantAdapterMismatch
            ))
        ));
        assert!(
            gateway
                .registry()
                .get(&AdapterId("sensor.registration".into()))
                .is_none()
        );
        assert!(gateway.adapter_sessions.is_empty());
    }

    assert!(
        schemas
            .get(&SchemaId("arcs.observation.registration.v1".into()))
            .is_none()
    );
}

#[test]
fn reasoning_registration_rejects_authoritative_schema_atomically() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();

    {
        let mut gateway = gateway(&mut schemas, &store);
        let adapter_id = AdapterId("reasoning.registration".into());
        let result = gateway.register_reasoning_adapter(
            Box::new(NeverCalledReasoner {
                manifest: reasoning_manifest("arcs.action.registration.v1"),
            }),
            reasoning_grant(),
            &[AUTHORITATIVE_ACTION_SCHEMA],
        );

        assert!(matches!(
            result,
            Err(AdapterGatewayError::ReasoningOutputMustBeCandidate(schema))
                if schema == SchemaId("arcs.action.registration.v1".into())
        ));
        assert!(gateway.registry().get(&adapter_id).is_none());
        assert!(!gateway.reasoning_endpoints.contains_key(&adapter_id));
        assert!(gateway.adapter_sessions.is_empty());
    }

    // Der Kandidaten-Check findet nach dem Schema-Staging statt. Gerade
    // deshalb muss auch dieser späte Fehler das gesamte Paket verwerfen.
    assert!(
        schemas
            .get(&SchemaId("arcs.action.registration.v1".into()))
            .is_none()
    );
}

#[test]
fn generic_registration_cannot_bypass_a_correlated_endpoint_boundary() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut gateway = gateway(&mut schemas, &store);

    let result = gateway.register_adapter(
        reasoning_manifest("arcs.action.registration.v1"),
        reasoning_grant(),
        &[AUTHORITATIVE_ACTION_SCHEMA],
    );

    assert!(matches!(
        result,
        Err(
            AdapterGatewayError::CapabilityRequiresDedicatedEndpoint(capability)
        ) if capability == CapabilityRef::new(
            "reasoning.registration",
            "reasoning.propose"
        )
    ));
    assert!(gateway.adapter_sessions.is_empty());
    assert!(
        gateway
            .registry()
            .get(&AdapterId("reasoning.registration".into()))
            .is_none()
    );
}
