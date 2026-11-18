use super::*;
use crate::adapters::{
    AdapterGrant, AdapterId, AdapterManifest, CapabilityContract, CapabilityDescription,
    CapabilityDescriptor, CapabilityId, DescriptionVerification,
};
use crate::core::{SchemaRegistry, SourceKind, TrustLevel};
use crate::reasoning::{ReasoningLimitsV2, ReasoningResources};
use crate::store::SqliteArtifactStore;
use serde_json::json;

fn query(value: serde_json::Value) -> CapabilityQuery {
    serde_json::from_value(value).unwrap()
}
fn install(
    registry: &mut AdapterRegistry,
    id: &str,
    contract: CapabilityContract,
    description: Option<serde_json::Value>,
) {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas
        .register_json(r#"{"$id":"arcs.test_candidate.v1","type":"object"}"#)
        .unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let reason = contract.is_reasoning();
    let ingress = matches!(
        contract,
        CapabilityContract::Input { .. }
            | CapabilityContract::Observe { .. }
            | CapabilityContract::Request { .. }
    );
    let capability = CapabilityDescriptor {
        id: CapabilityId("operation".into()),
        contract,
        required_permissions: vec!["use.adapter".into()],
        description: description
            .map(|value| serde_json::from_value::<CapabilityDescription>(value).unwrap()),
    };
    let mut disabled = capability.clone();
    disabled.id = CapabilityId("disabled".into());
    registry
        .register(
            AdapterManifest {
                protocol_version: 1,
                adapter_id: AdapterId(id.into()),
                adapter_version: "1".into(),
                capabilities: vec![capability, disabled],
            },
            AdapterGrant {
                adapter_id: AdapterId(id.into()),
                producer_class: ProducerClass::System,
                enabled_capabilities: vec![CapabilityId("operation".into())],
                granted_permissions: vec!["use.adapter".into()],
                assigned_trust: TrustLevel::High,
                ingress_source_kind: ingress.then_some(SourceKind::External),
                max_payload_bytes: 4096,
                max_external_reference_bytes: 256,
                data_limits: None,
                input_data_limits: None,
                reasoning_limits: None,
                reasoning_limits_v2: reason.then_some(ReasoningLimitsV2 {
                    methods: vec![ReasoningMethod::Rules],
                    resources: ReasoningResources {
                        max_context_items: 2,
                        max_context_bytes: 8192,
                        max_output_bytes: 4096,
                        max_candidates: 1,
                        max_runtime_ms: 1000,
                        max_iterations: Some(100),
                        max_output_tokens: None,
                    },
                }),
            },
            &schemas,
            &store,
        )
        .unwrap();
}
fn observe() -> CapabilityContract {
    CapabilityContract::Observe {
        emits: vec![SchemaId("arcs.input.v1".into())],
    }
}

#[test]
fn ranked_search_is_deterministic_and_keeps_claims_unverified_at_high_trust() {
    let mut registry = AdapterRegistry::new();
    install(
        &mut registry,
        "b",
        observe(),
        Some(json!({"purpose":"Read temperature","tags":["sensor"]})),
    );
    install(
        &mut registry,
        "a",
        observe(),
        Some(json!({"purpose":"Read temperature","tags":["sensor"]})),
    );
    install(
        &mut registry,
        "c",
        observe(),
        Some(json!({"purpose":"Sensor temperature reading"})),
    );
    let hits = registry
        .find_capabilities(&query(
            json!({"text":"SENSOR temperature","role":"observe"}),
        ))
        .unwrap();
    assert_eq!(
        hits.iter()
            .map(|hit| hit.capability.capability.adapter_id.0.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    assert!(
        hits.iter()
            .all(|hit| hit.capability.capability.capability_id.0 == "operation")
    );
    assert!(
        hits.iter()
            .all(|hit| hit.matched_terms == ["sensor", "temperature"])
    );
    assert!(hits.iter().all(|hit| {
        hit.capability
            .description_claim
            .as_ref()
            .unwrap()
            .verification
            == DescriptionVerification::Unverified
    }));
    assert!(hits[0].relevance_score > hits[2].relevance_score);
    assert!(
        registry
            .find_capabilities(&query(json!({"text":"sensor nonexistent"})))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn hard_contract_method_and_scope_filters_precede_ranking_and_limit() {
    let mut registry = AdapterRegistry::new();
    install(
        &mut registry,
        "observe",
        observe(),
        Some(json!({"purpose":"Find nearest number"})),
    );
    install(
        &mut registry,
        "reason",
        CapabilityContract::Reason {
            emits: vec![SchemaId("arcs.test_candidate.v1".into())],
        },
        Some(json!({"purpose":"Find nearest number"})),
    );
    let selected=registry.find_capabilities(&query(json!({"text":"nearest","role":"reason","emits_schema":"arcs.test_candidate.v1","reasoning_method":"rules","limit":1}))).unwrap();
    assert_eq!(
        selected[0].capability.capability,
        CapabilityRef::new("reason", "operation")
    );
    assert!(
        registry
            .find_capabilities(&query(json!({"reasoning_method":"learned"})))
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .find_capabilities(&query(json!({"accepts_schema":"arcs.input.v1"})))
            .unwrap()
            .is_empty()
    );
    assert!(
        registry
            .find_capabilities(&query(json!({"allowed_capabilities":[]})))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        registry
            .find_capabilities_with(&query(json!({"limit":1})), |cap| cap.adapter_id.0
                == "reason")
            .unwrap()[0]
            .capability
            .capability
            .adapter_id
            .0,
        "reason"
    );
    assert!(matches!(
        registry.find_capabilities(&query(
            json!({"allowed_capabilities":[{"adapter_id":"observe","capability_id":"disabled"}]})
        )),
        Err(CapabilityDiscoveryError::Authorization(_))
    ));
}

#[test]
fn invalid_queries_cannot_broaden_to_an_unfiltered_catalog() {
    let registry = AdapterRegistry::new();
    for value in [
        json!({"limit":0}),
        json!({"limit":65}),
        json!({"text":"!?!"}),
        json!({"text":"a\nb"}),
        json!({"text":"a".repeat(4097)}),
        json!({"text":(0..33).map(|n|n.to_string()).collect::<Vec<_>>().join(" ")}),
        json!({"reasoning_method":"rules","role":"act"}),
        json!({"emits_schema":""}),
        json!({"allowed_capabilities":[{"adapter_id":"a","capability_id":"b"},{"adapter_id":"a","capability_id":"b"}]}),
    ] {
        assert!(matches!(
            registry.find_capabilities(&query(value)),
            Err(CapabilityDiscoveryError::InvalidQuery(_))
        ));
    }
}

#[test]
fn old_descriptors_are_searchable_by_identity_without_fabricated_semantics() {
    let mut registry = AdapterRegistry::new();
    install(&mut registry, "legacy-sensor", observe(), None);
    let hits = registry
        .find_capabilities(&query(json!({"text":"legacy sensor"})))
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].capability.description_claim.is_none());
    assert!(
        serde_json::to_value(&hits[0].capability)
            .unwrap()
            .get("description_claim")
            .is_none()
    );
}
