use crate::adapters::*;
use crate::core::{SchemaId, SchemaRegistry, SourceKind, TrustLevel};
use crate::reasoning::{ReasoningLimitsV2, ReasoningMethod, ReasoningResources};
use crate::store::SqliteArtifactStore;
#[test]
fn all_seven_roles_support_multiple_installations_with_equal_local_names() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas
        .register_json(r#"{"$id":"arcs.test_candidate.v1","type":"object"}"#)
        .unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut registry = AdapterRegistry::new();
    let input = vec![SchemaId("arcs.input.v1".into())];
    let candidate = vec![SchemaId("arcs.test_candidate.v1".into())];
    let contracts = [
        CapabilityContract::Input {
            emits: input.clone(),
        },
        CapabilityContract::Observe {
            emits: input.clone(),
        },
        CapabilityContract::Request {
            accepts: input.clone(),
            emits: input.clone(),
        },
        CapabilityContract::Transform {
            accepts: input.clone(),
            emits: input.clone(),
        },
        CapabilityContract::Reason {
            emits: candidate.clone(),
        },
        CapabilityContract::Act {
            accepts: candidate,
            emits: input.clone(),
            idempotent: true,
        },
        CapabilityContract::Output {
            accepts: input.clone(),
            emits: input,
            idempotent: true,
        },
    ];
    for (index, contract) in contracts.iter().enumerate() {
        for suffix in ["b", "a"] {
            let id = AdapterId(format!("role-{index}-{suffix}"));
            let cap = CapabilityDescriptor {
                id: CapabilityId("same.name".into()),
                contract: contract.clone(),
                required_permissions: vec!["use.adapter".into()],
            };
            let mut disabled = cap.clone();
            disabled.id = CapabilityId("disabled".into());
            registry
                .register(
                    AdapterManifest {
                        protocol_version: 1,
                        adapter_id: id.clone(),
                        adapter_version: "1".into(),
                        capabilities: vec![cap, disabled],
                    },
                    AdapterGrant {
                        adapter_id: id,
                        producer_class: ProducerClass::Adapter,
                        enabled_capabilities: vec![CapabilityId("same.name".into())],
                        granted_permissions: vec!["use.adapter".into()],
                        assigned_trust: TrustLevel::Medium,
                        ingress_source_kind: if matches!(
                            contract,
                            CapabilityContract::Input { .. }
                                | CapabilityContract::Observe { .. }
                                | CapabilityContract::Request { .. }
                        ) {
                            Some(SourceKind::External)
                        } else {
                            None
                        },
                        max_payload_bytes: 4096,
                        max_external_reference_bytes: 256,
                        reasoning_limits: None,
                        reasoning_limits_v2: if contract.is_reasoning() {
                            Some(ReasoningLimitsV2 {
                                methods: vec![ReasoningMethod::Rules],
                                resources: ReasoningResources {
                                    max_context_items: 2,
                                    max_context_bytes: 4096,
                                    max_output_bytes: 4096,
                                    max_candidates: 1,
                                    max_runtime_ms: 1000,
                                    max_iterations: Some(100),
                                    max_output_tokens: None,
                                },
                            })
                        } else {
                            None
                        },
                        data_limits: None,
                        input_data_limits: None,
                    },
                    &schemas,
                    &store,
                )
                .unwrap();
        }
        assert_eq!(
            registry.capabilities(contract.role()),
            vec![
                CapabilityRef::new(format!("role-{index}-a"), "same.name"),
                CapabilityRef::new(format!("role-{index}-b"), "same.name")
            ]
        );
    }
}

#[test]
fn catalog_contains_only_enabled_capabilities_and_keeps_both_multi_role_connections() {
    use super::registry::AdapterRegistry;
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas
        .register_json(r#"{"$id":"arcs.test_candidate.v1","type":"object"}"#)
        .unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut registry = AdapterRegistry::new();
    for id in ["second", "first"] {
        let input = CapabilityDescriptor {
            id: CapabilityId("receive".into()),
            contract: CapabilityContract::Input {
                emits: vec![SchemaId("arcs.input.v1".into())],
            },
            required_permissions: vec![],
        };
        let mut disabled = input.clone();
        disabled.id = CapabilityId("disabled".into());
        let observe = CapabilityDescriptor {
            id: CapabilityId("observe".into()),
            contract: CapabilityContract::Observe {
                emits: vec![SchemaId("arcs.input.v1".into())],
            },
            required_permissions: vec![],
        };
        registry
            .register(
                AdapterManifest {
                    protocol_version: 1,
                    adapter_id: AdapterId(id.into()),
                    adapter_version: "1".into(),
                    capabilities: vec![input, disabled, observe],
                },
                AdapterGrant {
                    adapter_id: AdapterId(id.into()),
                    producer_class: ProducerClass::Adapter,
                    enabled_capabilities: vec![
                        CapabilityId("receive".into()),
                        CapabilityId("observe".into()),
                    ],
                    granted_permissions: vec![],
                    assigned_trust: TrustLevel::Medium,
                    ingress_source_kind: Some(SourceKind::External),
                    max_payload_bytes: 4096,
                    max_external_reference_bytes: 256,
                    reasoning_limits: None,
                    reasoning_limits_v2: None,
                    data_limits: None,
                    input_data_limits: None,
                },
                &schemas,
                &store,
            )
            .unwrap();
    }
    let catalog = registry.catalog();
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog[0].adapter_id.0, "first");
    assert_eq!(catalog[1].adapter_id.0, "second");
    for connection in catalog {
        assert_eq!(connection.capabilities.len(), 2);
        assert_eq!(connection.capabilities[0].role, AdapterRole::Observe);
        assert_eq!(connection.capabilities[1].role, AdapterRole::Input);
        assert!(
            connection
                .capabilities
                .iter()
                .all(|cap| cap.capability.adapter_id == connection.adapter_id
                    && cap.capability.capability_id.0 != "disabled")
        );
    }
}
