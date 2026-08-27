use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterCallError, AdapterGrant, AdapterId, AdapterManifest,
    AdapterRegistry, CapabilityContract, CapabilityDescriptor, CapabilityId, CapabilityRef,
    ProducerClass,
};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds,
    SchemaId, Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId,
};
use crate::reasoning::{
    ContextSelection, ReasoningAdapter, ReasoningBudget, ReasoningInvocation, ReasoningLimits,
    ReasoningRequest, ReasoningResponse, ReasoningTrace,
};
use crate::store::{ActiveSource, ArtifactNetwork, SqliteArtifactStore};

const CANDIDATE_SCHEMA: &str = r#"{
  "$id":"arcs.route_candidate.runtime_test.v1", "$schema":"https://json-schema.org/draft/2020-12/schema",
  "type":"object", "required":["summary"], "properties":{"summary":{"type":"string","minLength":1}}, "additionalProperties":false
}"#;

struct FixedClock;
impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-08-06T12:00:00Z".into()
    }
}

struct TestIds(u64);
impl ArtifactIdGenerator for TestIds {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
        let n = self.0;
        self.0 += 1;
        GeneratedArtifactIds {
            artifact_id: ArtifactId(format!("{artifact_type}-{n}")),
            version_id: VersionId(format!("{artifact_type}-{n}-v1")),
        }
    }
}

struct MockReasoner {
    manifest: AdapterManifest,
    calls: Arc<Mutex<usize>>,
    empty: bool,
}
impl ReasoningAdapter for MockReasoner {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }
    fn propose(
        &self,
        request: &ReasoningInvocation,
    ) -> Result<ReasoningResponse, AdapterCallError> {
        *self.calls.lock().unwrap() += 1;
        Ok(ReasoningResponse {
            invocation_id: request.invocation_id.clone(),
            request_id: request.request_id.clone(),
            candidates: if self.empty {
                vec![]
            } else {
                vec![crate::reasoning::ProposalSubmission {
                    schema_id: request.target_schema_id.clone(),
                    required_capabilities: vec![],
                    referenced_versions: request
                        .context
                        .iter()
                        .map(|item| item.version_id.clone())
                        .collect(),
                    payload: json!({"summary":"fallback"}),
                }]
            },
            trace: ReasoningTrace {
                model_name: "mock".into(),
                prompt_hash: "prompt".into(),
                raw_output_hash: "result".into(),
                temperature: 0.0,
            },
        })
    }
}

fn source() -> Artifact {
    Artifact::new(
        "source",
        "source-v1",
        "input",
        "arcs.input.v1",
        "2026-08-06T12:00:00Z",
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
        "source",
        json!({"raw_text":"help"}),
    )
}

fn known() -> Artifact {
    Artifact::new(
        "known",
        "known-v1",
        "route_candidate",
        "arcs.route_candidate.runtime_test.v1",
        "2026-08-06T12:00:00Z",
        Actor {
            actor_type: ActorType::System,
            id: "arcs".into(),
        },
        Source {
            kind: SourceKind::Internal,
            reference: "known".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        "known",
        json!({"summary":"known"}),
    )
}

fn manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("reasoner".into()),
        adapter_version: "1".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("propose".into()),
            contract: CapabilityContract::Reason {
                emits: vec![SchemaId("arcs.route_candidate.runtime_test.v1".into())],
            },
            required_permissions: vec![],
        }],
    }
}

fn grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("reasoner".into()),
        producer_class: ProducerClass::Model,
        enabled_capabilities: vec![CapabilityId("propose".into())],
        granted_permissions: vec![],
        assigned_trust: TrustLevel::Low,
        ingress_source_kind: None,
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        reasoning_limits: Some(ReasoningLimits {
            max_context_items: 2,
            max_context_bytes: 4096,
            max_output_tokens: 128,
            max_output_bytes: 4096,
            max_candidates: 2,
        }),
    }
}

fn request(input: &Artifact) -> ReasoningRequest {
    ReasoningRequest {
        request_id: "route-test".into(),
        reasoning_capability: CapabilityRef::new("reasoner", "propose"),
        objective: "resolve".into(),
        context: vec![ContextSelection {
            version_id: input.version_id.clone(),
            payload_fields: vec!["raw_text".into()],
        }],
        target_schema_id: SchemaId("arcs.route_candidate.runtime_test.v1".into()),
        allowed_capabilities: vec![],
        constraints: json!({}),
        budget: ReasoningBudget {
            max_context_items: 2,
            max_context_bytes: 4096,
            max_output_tokens: 64,
            max_output_bytes: 4096,
            max_candidates: 1,
        },
    }
}

fn policy() -> KnownRoutePolicy {
    KnownRoutePolicy {
        eligible_schema_ids: vec![SchemaId("arcs.route_candidate.runtime_test.v1".into())],
        minimum_trust: TrustLevel::Medium,
    }
}

fn setup(
    empty: bool,
) -> (
    crate::core::SchemaRegistry,
    SqliteArtifactStore,
    AdapterRegistry,
    Arc<Mutex<usize>>,
    AdapterManifest,
    Artifact,
) {
    let mut schemas = crate::core::SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(CANDIDATE_SCHEMA).unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = source();
    store.append(&input, &schemas).unwrap();
    let manifest = manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(manifest.clone(), grant(), &schemas, &store)
        .unwrap();
    let calls = Arc::new(Mutex::new(0));
    let _ = empty;
    (schemas, store, registry, calls, manifest, input)
}

fn resolve(
    empty: bool,
    connect_known: bool,
) -> (Result<RouteResolution, HybridRoutingError>, usize) {
    let (schemas, store, registry, calls, manifest, input) = setup(empty);
    if connect_known {
        let target = known();
        store.append(&target, &schemas).unwrap();
        ArtifactNetwork::new(&store)
            .connect(input.version_id.clone(), target.version_id, 1.0)
            .unwrap();
    }
    let endpoint = MockReasoner {
        manifest,
        calls: Arc::clone(&calls),
        empty,
    };
    let mut ids = TestIds(1);
    let mut reasoning = crate::reasoning::ReasoningService::new(
        &registry,
        &schemas,
        &store,
        &mut ids,
        &FixedClock,
        &endpoint,
    );
    let result = HybridRouter::new(&store, &mut reasoning).resolve(
        &[ActiveSource {
            version_id: input.version_id.clone(),
            activation: 1.0,
        }],
        0.5,
        &policy(),
        request(&input),
    );
    let count = *calls.lock().unwrap();
    (result, count)
}

#[test]
fn known_network_hit_skips_reasoning() {
    let (result, calls) = resolve(false, true);
    assert!(matches!(result, Ok(RouteResolution::KnownCandidates(_))));
    assert_eq!(calls, 0);
}

#[test]
fn unknown_case_calls_reasoning_once() {
    let (result, calls) = resolve(false, false);
    assert!(matches!(result, Ok(RouteResolution::ReasonedCandidates(_))));
    assert_eq!(calls, 1);
}

#[test]
fn empty_reasoning_response_is_unresolved() {
    let (result, calls) = resolve(true, false);
    assert!(matches!(result, Ok(RouteResolution::Unresolved)));
    assert_eq!(calls, 1);
}

#[test]
fn network_error_does_not_start_reasoning() {
    let (schemas, store, registry, calls, manifest, input) = setup(false);
    let endpoint = MockReasoner {
        manifest,
        calls: Arc::clone(&calls),
        empty: false,
    };
    let mut ids = TestIds(1);
    let mut reasoning = crate::reasoning::ReasoningService::new(
        &registry,
        &schemas,
        &store,
        &mut ids,
        &FixedClock,
        &endpoint,
    );
    let result = HybridRouter::new(&store, &mut reasoning).resolve(
        &[ActiveSource {
            version_id: VersionId("missing-v1".into()),
            activation: 1.0,
        }],
        0.5,
        &policy(),
        request(&input),
    );
    assert!(matches!(result, Err(HybridRoutingError::Network(_))));
    assert_eq!(*calls.lock().unwrap(), 0);
}

#[test]
fn invalid_route_policy_does_not_start_reasoning() {
    let (schemas, store, registry, calls, manifest, input) = setup(false);
    let endpoint = MockReasoner {
        manifest,
        calls: Arc::clone(&calls),
        empty: false,
    };
    let mut ids = TestIds(1);
    let mut reasoning = crate::reasoning::ReasoningService::new(
        &registry,
        &schemas,
        &store,
        &mut ids,
        &FixedClock,
        &endpoint,
    );
    let invalid = KnownRoutePolicy {
        eligible_schema_ids: vec![],
        minimum_trust: TrustLevel::Medium,
    };
    let result = HybridRouter::new(&store, &mut reasoning).resolve(
        &[ActiveSource {
            version_id: input.version_id.clone(),
            activation: 1.0,
        }],
        0.5,
        &invalid,
        request(&input),
    );
    assert!(matches!(result, Err(HybridRoutingError::Policy(_))));
    assert_eq!(*calls.lock().unwrap(), 0);
}
