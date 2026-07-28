use serde_json::json;

use super::*;
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, CapabilityContract,
    CapabilityDescriptor, CapabilityId, ObservationMessage, ProducerClass,
};
use crate::core::{
    SchemaId, SchemaRegistry, SequenceIdGenerator, SourceKind, SystemClock, TrustLevel,
};
use crate::store::SqliteArtifactStore;

fn manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("input.demo".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("input.observe".into()),
            contract: CapabilityContract::Observe {
                emits: vec![SchemaId("arcs.input.v1".into())],
            },
            required_permissions: vec![],
        }],
    }
}

fn grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("input.demo".into()),
        producer_class: ProducerClass::Adapter,
        enabled_capabilities: vec![CapabilityId("input.observe".into())],
        granted_permissions: vec![],
        assigned_trust: TrustLevel::Medium,
        observation_source_kind: Some(SourceKind::Chat),
        max_payload_bytes: 1024,
        max_external_reference_bytes: 256,
        reasoning_limits: None,
    }
}

#[test]
fn invalid_session_is_rejected_before_runtime_ingress() {
    let mut schemas_a = SchemaRegistry::with_bundled_schemas().unwrap();
    let store_a = SqliteArtifactStore::in_memory().unwrap();
    let mut gateway_a = AdapterGateway::new(
        &mut schemas_a,
        &store_a,
        Box::new(SystemClock),
        Box::new(SequenceIdGenerator::new("a")),
    );
    let foreign_session = gateway_a
        .register_adapter(manifest(), grant(), &[])
        .unwrap();

    let mut schemas_b = SchemaRegistry::with_bundled_schemas().unwrap();
    let store_b = SqliteArtifactStore::in_memory().unwrap();
    let mut gateway_b = AdapterGateway::new(
        &mut schemas_b,
        &store_b,
        Box::new(SystemClock),
        Box::new(SequenceIdGenerator::new("b")),
    );

    let result = gateway_b.ingest_observation(
        &foreign_session,
        ObservationMessage {
            capability_id: CapabilityId("input.observe".into()),
            external_subject: Some("request".into()),
            // Wäre die Runtime aufgerufen worden, wäre diese Referenz ungültig.
            external_reference: " ".into(),
            payload: json!({"raw_text": "must not be stored"}),
        },
    );

    assert!(matches!(
        result,
        Err(AdapterConnectionError::InvalidAdapterSession)
    ));
    assert!(store_b.is_empty().unwrap());
}
