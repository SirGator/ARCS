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
    SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId,
};
use crate::input::{InputMessage, InputService};
use crate::reasoning::{
    ContextSelection, ProposalSubmission, ReasoningAdapter, ReasoningBudget, ReasoningInvocation,
    ReasoningLimits, ReasoningResponse, ReasoningTrace,
};

const INPUT_ADAPTER: &str = "chat.agent-cycle-test";
const INPUT_CAPABILITY: &str = "chat.receive";
const REASONING_ADAPTER: &str = "reasoning.agent-cycle-test";
const REASONING_CAPABILITY: &str = "reasoning.propose";
const CANDIDATE_SCHEMA_ID: &str = "arcs.route_candidate.agent_cycle_input_test.v1";

const CANDIDATE_SCHEMA: &str = r#"{
    "$id": "arcs.route_candidate.agent_cycle_input_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["summary"],
    "properties": {"summary": {"type": "string", "minLength": 1}},
    "additionalProperties": false
}"#;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-08-08T12:00:00Z".into()
    }
}

struct TestIds(u64);

impl ArtifactIdGenerator for TestIds {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
        let sequence = self.0;
        self.0 += 1;
        GeneratedArtifactIds {
            artifact_id: ArtifactId(format!("{artifact_type}-{sequence}")),
            version_id: VersionId(format!("{artifact_type}-{sequence}-v1")),
        }
    }
}

struct RecordingReasoner {
    manifest: AdapterManifest,
    calls: Arc<Mutex<Vec<ReasoningInvocation>>>,
}

impl ReasoningAdapter for RecordingReasoner {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn propose(
        &self,
        request: &ReasoningInvocation,
    ) -> Result<ReasoningResponse, AdapterCallError> {
        self.calls.lock().unwrap().push(request.clone());
        Ok(ReasoningResponse {
            invocation_id: request.invocation_id.clone(),
            request_id: request.request_id.clone(),
            candidates: vec![ProposalSubmission {
                schema_id: request.target_schema_id.clone(),
                required_capabilities: vec![],
                referenced_versions: request
                    .context
                    .iter()
                    .map(|item| item.version_id.clone())
                    .collect(),
                payload: json!({"summary": "reasoned route"}),
            }],
            trace: ReasoningTrace {
                model_name: "recording-reasoner".into(),
                prompt_hash: "prompt-hash".into(),
                raw_output_hash: "output-hash".into(),
                temperature: 0.0,
            },
        })
    }
}

fn input_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId(INPUT_ADAPTER.into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId(INPUT_CAPABILITY.into()),
            contract: CapabilityContract::Input {
                emits: vec![SchemaId("arcs.input.v1".into())],
            },
            required_permissions: vec![],
        }],
    }
}

fn input_grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId(INPUT_ADAPTER.into()),
        producer_class: ProducerClass::Adapter,
        enabled_capabilities: vec![CapabilityId(INPUT_CAPABILITY.into())],
        granted_permissions: vec![],
        assigned_trust: TrustLevel::Medium,
        ingress_source_kind: Some(SourceKind::Chat),
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        reasoning_limits: None,
    }
}

fn reasoning_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId(REASONING_ADAPTER.into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId(REASONING_CAPABILITY.into()),
            contract: CapabilityContract::Reason {
                emits: vec![SchemaId(CANDIDATE_SCHEMA_ID.into())],
            },
            required_permissions: vec![],
        }],
    }
}

fn reasoning_grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId(REASONING_ADAPTER.into()),
        producer_class: ProducerClass::Model,
        enabled_capabilities: vec![CapabilityId(REASONING_CAPABILITY.into())],
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

fn setup() -> (
    SchemaRegistry,
    SqliteArtifactStore,
    AdapterRegistry,
    AdapterManifest,
) {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(CANDIDATE_SCHEMA).unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let reasoner_manifest = reasoning_manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(input_manifest(), input_grant(), &schemas, &store)
        .unwrap();
    registry
        .register(
            reasoner_manifest.clone(),
            reasoning_grant(),
            &schemas,
            &store,
        )
        .unwrap();
    (schemas, store, registry, reasoner_manifest)
}

fn ingest_input(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
) -> Artifact {
    let mut ids = TestIds(1);
    InputService::new(registry, schemas, store, &mut ids, &FixedClock)
        .ingest(
            &AdapterId(INPUT_ADAPTER.into()),
            InputMessage {
                capability_id: CapabilityId(INPUT_CAPABILITY.into()),
                external_subject: Some("conversation-20".into()),
                external_reference: "chat://conversation-20/message-1".into(),
                payload: json!({"raw_text": "where is room 20?"}),
            },
        )
        .unwrap()
}

fn known_target() -> Artifact {
    Artifact::new(
        "known-route",
        "known-route-v1",
        "route_candidate",
        CANDIDATE_SCHEMA_ID,
        "2026-08-08T12:00:00Z",
        Actor {
            actor_type: ActorType::System,
            id: "arcs.test".into(),
        },
        Source {
            kind: SourceKind::Internal,
            reference: "known-route".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        "known-route",
        json!({"summary": "known route"}),
    )
}

fn reasoning_request(input: &Artifact) -> ReasoningRequest {
    ReasoningRequest {
        request_id: "agent-cycle-input-route".into(),
        reasoning_capability: CapabilityRef::new(REASONING_ADAPTER, REASONING_CAPABILITY),
        objective: "resolve input route".into(),
        context: vec![ContextSelection {
            version_id: input.version_id.clone(),
            payload_fields: vec!["raw_text".into()],
        }],
        target_schema_id: SchemaId(CANDIDATE_SCHEMA_ID.into()),
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

fn known_route_policy() -> KnownRoutePolicy {
    KnownRoutePolicy {
        eligible_schema_ids: vec![SchemaId(CANDIDATE_SCHEMA_ID.into())],
        minimum_trust: TrustLevel::Medium,
    }
}

#[test]
fn input_artifact_can_drive_known_network_route() {
    let (schemas, store, registry, reasoner_manifest) = setup();
    let input = ingest_input(&registry, &schemas, &store);
    let target = known_target();
    store.append(&target, &schemas).unwrap();
    ArtifactNetwork::new(&store)
        .connect(input.version_id.clone(), target.version_id.clone(), 1.0)
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let endpoint = RecordingReasoner {
        manifest: reasoner_manifest,
        calls: Arc::clone(&calls),
    };
    let mut ids = TestIds(100);
    let mut reasoning = ReasoningService::new(
        &registry,
        &schemas,
        &store,
        &mut ids,
        &FixedClock,
        &endpoint,
    );
    let source = ActiveSource {
        version_id: input.version_id.clone(),
        activation: 1.0,
    };

    let result = AgentCycle::new(&store)
        .resolve_with_fallback(
            &mut reasoning,
            &[source],
            0.5,
            &known_route_policy(),
            reasoning_request(&input),
        )
        .unwrap();

    assert!(matches!(result, RouteResolution::KnownCandidates(_)));
    assert_eq!(calls.lock().unwrap().len(), 0);
}

#[test]
fn unknown_input_falls_back_to_reasoning() {
    let (schemas, store, registry, reasoner_manifest) = setup();
    let input = ingest_input(&registry, &schemas, &store);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let endpoint = RecordingReasoner {
        manifest: reasoner_manifest,
        calls: Arc::clone(&calls),
    };
    let mut ids = TestIds(100);
    let mut reasoning = ReasoningService::new(
        &registry,
        &schemas,
        &store,
        &mut ids,
        &FixedClock,
        &endpoint,
    );
    let source = ActiveSource {
        version_id: input.version_id.clone(),
        activation: 1.0,
    };
    let request = reasoning_request(&input);
    assert_eq!(request.context[0].version_id, input.version_id);

    let result = AgentCycle::new(&store)
        .resolve_with_fallback(
            &mut reasoning,
            &[source],
            0.5,
            &known_route_policy(),
            request,
        )
        .unwrap();

    assert!(matches!(result, RouteResolution::ReasonedCandidates(_)));
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].context[0].version_id, input.version_id);
}
