use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use serde_json::json;

use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterCallError, AdapterGrant, AdapterId, AdapterManifest,
    CapabilityContract, CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
    ReasoningAdapter, ReasoningBudget, ReasoningInvocation, ReasoningLimits, ReasoningResponse,
    ReasoningTrace,
};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds,
    SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId,
};
use crate::store::SqliteArtifactStore;

use super::*;

const CANDIDATE_SCHEMA: &str = r#"{
    "$id": "arcs.route_candidate.runtime.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "properties": {},
    "additionalProperties": false
}"#;

const KNOWN_ROUTE_SCHEMA: &str = r#"{
    "$id": "arcs.known_route.demo.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "properties": {},
    "additionalProperties": false
}"#;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-07-27T12:00:00Z".into()
    }
}

struct FixedIds;

impl ArtifactIdGenerator for FixedIds {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
        GeneratedArtifactIds {
            artifact_id: ArtifactId(format!("{artifact_type}-candidate")),
            version_id: VersionId(format!("{artifact_type}-candidate-v1")),
        }
    }
}

struct CountingReasoner {
    manifest: AdapterManifest,
    calls: Arc<AtomicUsize>,
}

impl ReasoningAdapter for CountingReasoner {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn propose(
        &self,
        request: &ReasoningInvocation,
    ) -> Result<ReasoningResponse, AdapterCallError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ReasoningResponse {
            request_id: request.request_id.clone(),
            candidates: vec![],
            trace: ReasoningTrace {
                model_name: "mock-model".into(),
                prompt_hash: "prompt".into(),
                raw_output_hash: "output".into(),
                temperature: 0.0,
            },
        })
    }
}

fn input(id: &str) -> Artifact {
    Artifact::new(
        id,
        format!("{id}-v1"),
        "input",
        "arcs.input.v1",
        "2026-07-27T12:00:00Z",
        Actor {
            actor_type: ActorType::Human,
            id: "user".into(),
        },
        Source {
            kind: SourceKind::Chat,
            reference: "chat".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::Human,
        },
        "test",
        json!({"raw_text": id}),
    )
}

fn reasoning_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("reasoning.mock".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("reasoning.propose".into()),
            contract: CapabilityContract::Reason {
                emits: vec![SchemaId("arcs.route_candidate.runtime.v1".into())],
            },
            required_permissions: vec![],
        }],
    }
}

fn reasoning_request() -> ReasoningRequest {
    ReasoningRequest {
        request_id: "fallback-1".into(),
        reasoning_capability: CapabilityRef::new("reasoning.mock", "reasoning.propose"),
        objective: "Handle unknown input".into(),
        context: vec![],
        target_schema_id: SchemaId("arcs.route_candidate.runtime.v1".into()),
        allowed_capabilities: vec![],
        constraints: json!({}),
        budget: ReasoningBudget {
            max_context_items: 1,
            max_context_bytes: 2048,
            max_output_tokens: 128,
            max_output_bytes: 2048,
            max_candidates: 1,
        },
    }
}

fn reasoning_grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("reasoning.mock".into()),
        producer_class: ProducerClass::Model,
        enabled_capabilities: vec![CapabilityId("reasoning.propose".into())],
        granted_permissions: vec![],
        assigned_trust: TrustLevel::Low,
        observation_source_kind: None,
        max_payload_bytes: 2048,
        max_external_reference_bytes: 256,
        reasoning_limits: Some(ReasoningLimits {
            max_context_items: 2,
            max_context_bytes: 4096,
            max_output_tokens: 256,
            max_output_bytes: 4096,
            max_candidates: 2,
        }),
    }
}

fn input_route_policy() -> KnownRoutePolicy {
    KnownRoutePolicy {
        eligible_schema_ids: vec![SchemaId("arcs.input.v1".into())],
        minimum_trust: TrustLevel::High,
    }
}

#[test]
fn known_route_skips_reasoner_and_unknown_route_calls_it_once() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let source = input("source");
    let target = input("target");
    let unknown = input("unknown");
    for artifact in [&source, &target, &unknown] {
        store.append(artifact, &schemas).unwrap();
    }
    ArtifactNetwork::new(&store)
        .connect(source.version_id.clone(), target.version_id.clone(), 0.9)
        .unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let mut gateway = AdapterGateway::new(
        &mut schemas,
        &store,
        Box::new(FixedClock),
        Box::new(FixedIds),
    );
    gateway
        .register_reasoning_adapter(
            Box::new(CountingReasoner {
                manifest: reasoning_manifest(),
                calls: calls.clone(),
            }),
            reasoning_grant(),
            &[CANDIDATE_SCHEMA],
        )
        .unwrap();

    let mut router = HybridRouter::new(&mut gateway);
    let known = router
        .resolve(
            &[ActiveSource {
                version_id: source.version_id,
                activation: 1.0,
            }],
            0.5,
            &input_route_policy(),
            reasoning_request(),
        )
        .unwrap();
    assert!(matches!(known, RouteResolution::KnownCandidates(routes) if routes.len() == 1));
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let unknown = router
        .resolve(
            &[ActiveSource {
                version_id: unknown.version_id,
                activation: 1.0,
            }],
            0.5,
            &input_route_policy(),
            reasoning_request(),
        )
        .unwrap();
    assert!(matches!(unknown, RouteResolution::Unresolved));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn non_route_network_target_does_not_suppress_fallback() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(KNOWN_ROUTE_SCHEMA).unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let source = input("source");
    let observation = input("observation");
    for artifact in [&source, &observation] {
        store.append(artifact, &schemas).unwrap();
    }
    ArtifactNetwork::new(&store)
        .connect(
            source.version_id.clone(),
            observation.version_id.clone(),
            0.9,
        )
        .unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let mut gateway = AdapterGateway::new(
        &mut schemas,
        &store,
        Box::new(FixedClock),
        Box::new(FixedIds),
    );
    gateway
        .register_reasoning_adapter(
            Box::new(CountingReasoner {
                manifest: reasoning_manifest(),
                calls: calls.clone(),
            }),
            reasoning_grant(),
            &[CANDIDATE_SCHEMA],
        )
        .unwrap();
    let route_only = KnownRoutePolicy {
        eligible_schema_ids: vec![SchemaId("arcs.known_route.demo.v1".into())],
        minimum_trust: TrustLevel::Low,
    };

    let result = HybridRouter::new(&mut gateway)
        .resolve(
            &[ActiveSource {
                version_id: source.version_id,
                activation: 1.0,
            }],
            0.5,
            &route_only,
            reasoning_request(),
        )
        .unwrap();

    assert!(matches!(result, RouteResolution::Unresolved));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn invalid_route_policy_fails_before_network_or_reasoner() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut gateway = AdapterGateway::new(
        &mut schemas,
        &store,
        Box::new(FixedClock),
        Box::new(FixedIds),
    );
    gateway
        .register_reasoning_adapter(
            Box::new(CountingReasoner {
                manifest: reasoning_manifest(),
                calls: calls.clone(),
            }),
            reasoning_grant(),
            &[CANDIDATE_SCHEMA],
        )
        .unwrap();

    let result = HybridRouter::new(&mut gateway).resolve(
        &[ActiveSource {
            version_id: VersionId("missing-source-v1".into()),
            activation: 1.0,
        }],
        0.5,
        &KnownRoutePolicy {
            eligible_schema_ids: vec![],
            minimum_trust: TrustLevel::Low,
        },
        reasoning_request(),
    );

    assert!(matches!(
        result,
        Err(HybridRoutingError::Policy(
            KnownRoutePolicyError::EmptyEligibleSchemas
        ))
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn route_policy_rejects_duplicate_and_unregistered_schemas() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let input_schema = SchemaId("arcs.input.v1".into());

    assert_eq!(
        KnownRoutePolicy {
            eligible_schema_ids: vec![input_schema.clone(), input_schema.clone()],
            minimum_trust: TrustLevel::Low,
        }
        .validate(&schemas),
        Err(KnownRoutePolicyError::DuplicateSchema(input_schema))
    );
    let unknown = SchemaId("arcs.route.unknown.v1".into());
    assert_eq!(
        KnownRoutePolicy {
            eligible_schema_ids: vec![unknown.clone()],
            minimum_trust: TrustLevel::Low,
        }
        .validate(&schemas),
        Err(KnownRoutePolicyError::UnregisteredSchema(unknown))
    );
}

#[test]
fn network_error_never_triggers_reasoning_fallback() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut gateway = AdapterGateway::new(
        &mut schemas,
        &store,
        Box::new(FixedClock),
        Box::new(FixedIds),
    );
    gateway
        .register_reasoning_adapter(
            Box::new(CountingReasoner {
                manifest: reasoning_manifest(),
                calls: calls.clone(),
            }),
            reasoning_grant(),
            &[CANDIDATE_SCHEMA],
        )
        .unwrap();

    let mut router = HybridRouter::new(&mut gateway);
    let result = router.resolve(
        &[ActiveSource {
            version_id: VersionId("source-v1".into()),
            activation: 2.0,
        }],
        0.5,
        &input_route_policy(),
        reasoning_request(),
    );

    assert!(matches!(result, Err(HybridRoutingError::Network(_))));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
