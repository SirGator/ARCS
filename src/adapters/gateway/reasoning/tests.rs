use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterCallError, AdapterId, ArtifactIdGenerator, CapabilityContract,
    CapabilityDescriptor, CapabilityId, ContextSelection, GeneratedArtifactIds, ProposalSubmission,
    ReasoningAdapter, ReasoningBudget, ReasoningLimits, ReasoningResponse, ReasoningTrace,
};
use crate::core::{
    Actor, ActorType, ArtifactId, Source, SourceClass, SourceKind, Trust, TrustLevel,
};
use crate::store::{ArtifactNetwork, SqliteArtifactStore};

const ROUTE_SCHEMA: &str = r#"{
    "$id": "arcs.route_candidate.demo.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["summary", "capability"],
    "properties": {
        "summary": {"type": "string", "minLength": 1},
        "capability": {"type": "string", "minLength": 1}
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

struct MockReasoner {
    manifest: AdapterManifest,
    response: ReasoningResponse,
    calls: Arc<Mutex<Vec<ReasoningInvocation>>>,
}

impl ReasoningAdapter for MockReasoner {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn propose(
        &self,
        request: &ReasoningInvocation,
    ) -> Result<ReasoningResponse, AdapterCallError> {
        self.calls.lock().unwrap().push(request.clone());
        Ok(self.response.clone())
    }
}

/// Test-Endpoint mit zwei Reasoning-Capabilities unter derselben Adapter-ID.
///
/// Die Antwort wird bewusst aus der Wire-Capability abgeleitet. Damit beweist
/// der Test nicht nur, dass der Core ein Feld befüllt, sondern dass ein realer
/// Multi-Capability-Endpoint seine interne Operation danach auswählen kann.
struct CapabilityAwareReasoner {
    manifest: AdapterManifest,
    calls: Arc<Mutex<Vec<ReasoningInvocation>>>,
}

impl ReasoningAdapter for CapabilityAwareReasoner {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn propose(
        &self,
        request: &ReasoningInvocation,
    ) -> Result<ReasoningResponse, AdapterCallError> {
        self.calls.lock().unwrap().push(request.clone());
        let mode = request.capability.capability_id.0.clone();
        Ok(ReasoningResponse {
            request_id: request.request_id.clone(),
            candidates: vec![ProposalSubmission {
                schema_id: request.target_schema_id.clone(),
                required_capabilities: vec![CapabilityRef::new("action.demo", "device.set")],
                referenced_versions: vec![request.context[0].version_id.clone()],
                payload: json!({
                    "summary": format!("selected by {mode}"),
                    "capability": "device.set"
                }),
            }],
            trace: ReasoningTrace {
                model_name: format!("mock-{mode}"),
                prompt_hash: "prompt-sha256".into(),
                raw_output_hash: "output-sha256".into(),
                temperature: 0.0,
            },
        })
    }
}

struct UnavailableReasoner {
    manifest: AdapterManifest,
}

impl ReasoningAdapter for UnavailableReasoner {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn propose(
        &self,
        _request: &ReasoningInvocation,
    ) -> Result<ReasoningResponse, AdapterCallError> {
        Err(AdapterCallError::Unavailable("model offline".into()))
    }
}

fn gateway<'a>(
    schemas: &'a mut SchemaRegistry,
    store: &'a SqliteArtifactStore,
) -> AdapterGateway<'a> {
    AdapterGateway::new(schemas, store, Box::new(FixedClock), Box::new(TestIds(1)))
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
        observation_source_kind: None,
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        reasoning_limits: (producer_class == ProducerClass::Model).then_some(ReasoningLimits {
            max_context_items: 8,
            max_context_bytes: 8192,
            max_output_tokens: 1024,
            max_output_bytes: 8192,
            max_candidates: 8,
        }),
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

fn reasoning_manifest(output_schema: &str) -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("reasoning.mock".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("reasoning.propose".into()),
            contract: CapabilityContract::Reason {
                emits: vec![SchemaId(output_schema.into())],
            },
            required_permissions: vec![],
        }],
    }
}

fn multi_capability_reasoning_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("reasoning.mock".into()),
        adapter_version: "1.0.0".into(),
        capabilities: ["reasoning.fast", "reasoning.deep"]
            .into_iter()
            .map(|id| CapabilityDescriptor {
                id: CapabilityId(id.into()),
                contract: CapabilityContract::Reason {
                    emits: vec![SchemaId("arcs.route_candidate.demo.v1".into())],
                },
                required_permissions: vec![],
            })
            .collect(),
    }
}

fn input_artifact() -> Artifact {
    Artifact::new(
        "input-1",
        "input-1-v1",
        "input",
        "arcs.input.v1",
        "2026-07-27T11:59:00Z",
        Actor {
            actor_type: ActorType::Human,
            id: "user-1".into(),
        },
        Source {
            kind: SourceKind::Chat,
            reference: "private-conversation-reference".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::Human,
        },
        "conversation-1",
        json!({"raw_text": "Turn on the light"}),
    )
}

fn request(context: VersionId) -> ReasoningRequest {
    ReasoningRequest {
        request_id: "reason-1".into(),
        reasoning_capability: CapabilityRef::new("reasoning.mock", "reasoning.propose"),
        objective: "Choose a safe known capability".into(),
        context: vec![ContextSelection {
            version_id: context,
            payload_fields: vec!["raw_text".into()],
        }],
        target_schema_id: SchemaId("arcs.route_candidate.demo.v1".into()),
        allowed_capabilities: vec![CapabilityRef::new("action.demo", "device.set")],
        constraints: json!({"maximum_risk": "low"}),
        budget: ReasoningBudget {
            max_context_items: 4,
            max_context_bytes: 4096,
            max_output_tokens: 512,
            max_output_bytes: 4096,
            max_candidates: 3,
        },
    }
}

fn valid_response(input: &VersionId) -> ReasoningResponse {
    ReasoningResponse {
        request_id: "reason-1".into(),
        candidates: vec![ProposalSubmission {
            schema_id: SchemaId("arcs.route_candidate.demo.v1".into()),
            required_capabilities: vec![CapabilityRef::new("action.demo", "device.set")],
            referenced_versions: vec![input.clone()],
            payload: json!({
                "summary": "Use the registered device capability",
                "capability": "device.set"
            }),
        }],
        trace: ReasoningTrace {
            model_name: "mock-model".into(),
            prompt_hash: "prompt-sha256".into(),
            raw_output_hash: "output-sha256".into(),
            temperature: 0.0,
        },
    }
}

fn register_action(gateway: &mut AdapterGateway<'_>) {
    gateway
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
}

#[test]
fn stores_auditable_request_but_keeps_proposal_out_of_store_and_network() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = input_artifact();
    store.append(&input, &schemas).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut gateway = gateway(&mut schemas, &store);

    register_action(&mut gateway);
    gateway
        .register_reasoning_adapter(
            Box::new(MockReasoner {
                manifest: reasoning_manifest("arcs.route_candidate.demo.v1"),
                response: valid_response(&input.version_id),
                calls: calls.clone(),
            }),
            grant(
                "reasoning.mock",
                ProducerClass::Model,
                &["reasoning.propose"],
                &[],
            ),
            &[],
        )
        .unwrap();

    let proposals = gateway.reason(request(input.version_id.clone())).unwrap();

    assert_eq!(proposals.len(), 1);
    assert_eq!(
        proposals[0].required_capabilities(),
        &[CapabilityRef::new("action.demo", "device.set")]
    );
    assert_eq!(store.len().unwrap(), 2);
    let reasoning_request = store
        .get(proposals[0].reasoning_request_version())
        .unwrap()
        .expect("ReasoningRequest muss vor dem Modellaufruf gespeichert sein");
    assert_eq!(
        reasoning_request.schema_id,
        SchemaId("arcs.reasoning_request.v1".into())
    );
    assert_eq!(reasoning_request.created_by.actor_type, ActorType::System);
    assert_eq!(
        reasoning_request.payload,
        json!({
            "objective": "Choose a safe known capability",
            "context_refs": [input.version_id.0.clone()]
        })
    );

    let captured = calls.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(
        captured[0].capability,
        CapabilityRef::new("reasoning.mock", "reasoning.propose")
    );
    assert_eq!(
        captured[0].context[0].payload,
        json!({"raw_text": "Turn on the light"})
    );
    let wire = serde_json::to_string(&captured[0]).unwrap();
    assert!(!wire.contains("private-conversation-reference"));
    assert!(!wire.contains("created_by"));
    drop(captured);

    // Ein Modellvorschlag ist weder Persistenz noch Netzverbindung und
    // damit insbesondere noch keine autorisierte Aktion.
    let network = ArtifactNetwork::new(&store);
    assert!(network.neighbors(&input.version_id).unwrap().is_empty());
}

#[test]
fn multi_capability_endpoint_receives_exact_authorized_capability() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = input_artifact();
    store.append(&input, &schemas).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut gateway = gateway(&mut schemas, &store);

    register_action(&mut gateway);
    gateway
        .register_reasoning_adapter(
            Box::new(CapabilityAwareReasoner {
                manifest: multi_capability_reasoning_manifest(),
                calls: calls.clone(),
            }),
            grant(
                "reasoning.mock",
                ProducerClass::Model,
                &["reasoning.fast", "reasoning.deep"],
                &[],
            ),
            &[],
        )
        .unwrap();

    // Dieselbe fachliche Request-ID ist pro konkreter Capability eindeutig.
    // Deshalb dürfen zwei verschiedene, autorisierte Ports sie unabhängig
    // verwenden; ein zweiter Aufruf desselben Ports bleibt weiterhin gesperrt.
    let mut fast = request(input.version_id.clone());
    fast.reasoning_capability = CapabilityRef::new("reasoning.mock", "reasoning.fast");
    let fast_proposal = gateway.reason(fast).unwrap();

    let mut deep = request(input.version_id.clone());
    deep.reasoning_capability = CapabilityRef::new("reasoning.mock", "reasoning.deep");
    let deep_proposal = gateway.reason(deep).unwrap();

    let mut duplicate_fast = request(input.version_id);
    duplicate_fast.reasoning_capability = CapabilityRef::new("reasoning.mock", "reasoning.fast");
    let duplicate = gateway.reason(duplicate_fast);
    assert!(matches!(
        duplicate,
        Err(AdapterGatewayError::ReasoningRequestAlreadyUsed {
            capability,
            request_id,
        }) if capability == CapabilityRef::new("reasoning.mock", "reasoning.fast")
            && request_id == "reason-1"
    ));

    assert_eq!(
        fast_proposal[0].payload["summary"],
        json!("selected by reasoning.fast")
    );
    assert_eq!(
        deep_proposal[0].payload["summary"],
        json!("selected by reasoning.deep")
    );
    let captured = calls.lock().unwrap();
    assert_eq!(
        captured
            .iter()
            .map(|invocation| invocation.capability.clone())
            .collect::<Vec<_>>(),
        vec![
            CapabilityRef::new("reasoning.mock", "reasoning.fast"),
            CapabilityRef::new("reasoning.mock", "reasoning.deep"),
        ]
    );
    assert_eq!(store.len().unwrap(), 3);
}

#[test]
fn rejects_schema_valid_candidate_with_capability_outside_request_allowlist() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = input_artifact();
    store.append(&input, &schemas).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut gateway = gateway(&mut schemas, &store);

    register_action(&mut gateway);
    let mut response = valid_response(&input.version_id);
    response.candidates[0].required_capabilities =
        vec![CapabilityRef::new("action.demo", "admin.delete_all")];
    response.candidates[0].payload["capability"] = json!("admin.delete_all");
    gateway
        .register_reasoning_adapter(
            Box::new(MockReasoner {
                manifest: reasoning_manifest("arcs.route_candidate.demo.v1"),
                response,
                calls,
            }),
            grant(
                "reasoning.mock",
                ProducerClass::Model,
                &["reasoning.propose"],
                &[],
            ),
            &[],
        )
        .unwrap();

    let result = gateway.reason(request(input.version_id));

    assert!(matches!(
        result,
        Err(AdapterGatewayError::ForbiddenCandidateCapability(_))
    ));
    // Der externe Aufruf wurde auditierbar begonnen; sein anschließend
    // verworfener Kandidat gelangt dagegen nicht in den Store.
    assert_eq!(store.len().unwrap(), 2);
}

#[test]
fn rejects_budget_above_operator_ceiling_before_context_lookup_or_model_call() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut gateway = gateway(&mut schemas, &store);
    gateway
        .register_reasoning_adapter(
            Box::new(MockReasoner {
                manifest: reasoning_manifest("arcs.route_candidate.demo.v1"),
                response: ReasoningResponse {
                    request_id: "reason-1".into(),
                    candidates: vec![],
                    trace: ReasoningTrace {
                        model_name: "unused".into(),
                        prompt_hash: "unused".into(),
                        raw_output_hash: "unused".into(),
                        temperature: 0.0,
                    },
                },
                calls: calls.clone(),
            }),
            grant(
                "reasoning.mock",
                ProducerClass::Model,
                &["reasoning.propose"],
                &[],
            ),
            &[ROUTE_SCHEMA],
        )
        .unwrap();
    let mut oversized = request(VersionId("missing-context".into()));
    oversized.allowed_capabilities.clear();
    oversized.budget.max_output_tokens = 1025;

    let result = gateway.reason(oversized);

    assert!(matches!(
        result,
        Err(AdapterGatewayError::ReasoningBudgetExceedsGrant)
    ));
    assert!(calls.lock().unwrap().is_empty());
    assert!(store.is_empty().unwrap());
}

#[test]
fn persists_reasoning_request_before_a_failing_external_call() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = input_artifact();
    store.append(&input, &schemas).unwrap();
    let mut gateway = gateway(&mut schemas, &store);

    register_action(&mut gateway);
    gateway
        .register_reasoning_adapter(
            Box::new(UnavailableReasoner {
                manifest: reasoning_manifest("arcs.route_candidate.demo.v1"),
            }),
            grant(
                "reasoning.mock",
                ProducerClass::Model,
                &["reasoning.propose"],
                &[],
            ),
            &[],
        )
        .unwrap();

    let result = gateway.reason(request(input.version_id.clone()));

    assert!(matches!(
        result,
        Err(AdapterGatewayError::AdapterCall(
            AdapterCallError::Unavailable(_)
        ))
    ));
    let audit = store
        .get(&VersionId("reasoning_request-1-v1".into()))
        .unwrap()
        .expect("Audit-Artifact muss trotz Transportfehler bestehen bleiben");
    assert_eq!(audit.payload["context_refs"], json!([input.version_id.0]));
    assert_eq!(store.len().unwrap(), 2);
}

#[test]
fn rejects_reasoner_that_declares_authoritative_output_schema() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut gateway = gateway(&mut schemas, &store);

    let result = gateway.register_reasoning_adapter(
        Box::new(MockReasoner {
            manifest: reasoning_manifest("arcs.input.v1"),
            response: ReasoningResponse {
                request_id: "unused".into(),
                candidates: vec![],
                trace: ReasoningTrace {
                    model_name: "unused".into(),
                    prompt_hash: "unused".into(),
                    raw_output_hash: "unused".into(),
                    temperature: 0.0,
                },
            },
            calls,
        }),
        grant(
            "reasoning.mock",
            ProducerClass::Model,
            &["reasoning.propose"],
            &[],
        ),
        &[],
    );

    assert!(matches!(
        result,
        Err(AdapterGatewayError::ReasoningOutputMustBeCandidate(schema))
            if schema == SchemaId("arcs.input.v1".into())
    ));
    assert!(
        gateway
            .registry()
            .get(&AdapterId("reasoning.mock".into()))
            .is_none()
    );
}
