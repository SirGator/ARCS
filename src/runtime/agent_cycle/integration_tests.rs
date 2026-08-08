use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;
use crate::action::ActionService;
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, ActAdapter, ActInvocation, ActResponse, AdapterCallError,
    AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry, CapabilityContract,
    CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use crate::approval::{ApprovalDecision, ApprovalError, ApprovalService};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds,
    SchemaId, SchemaRegistry, SourceKind, TrustLevel, VersionId,
};
use crate::execution::ExecutionService;
use crate::input::{InputMessage, InputService};
use crate::reasoning::{
    ContextSelection, ProposalSubmission, ReasoningAdapter, ReasoningBudget, ReasoningInvocation,
    ReasoningLimits, ReasoningResponse, ReasoningService, ReasoningTrace,
};
use crate::runtime::{KnownRoutePolicy, RouteResolution};
use crate::store::{ActiveSource, RelationKind, SqliteArtifactStore, relation_kinds};
use crate::verification::{
    VerificationError, VerificationFinding, VerificationResult, VerificationService,
    VerificationVerdict, Verifier,
};

const INPUT_ADAPTER: &str = "chat.vertical-test";
const INPUT_CAPABILITY: &str = "chat.receive";
const REASONING_ADAPTER: &str = "reasoning.vertical-test";
const REASONING_CAPABILITY: &str = "reasoning.propose";
const ACT_ADAPTER: &str = "executor.vertical-test";
const ACT_CAPABILITY: &str = "door.open";
const CANDIDATE_SCHEMA_ID: &str = "arcs.action_candidate.vertical_test.v1";
const RESULT_SCHEMA_ID: &str = "arcs.execution_result.vertical_test.v1";

const CANDIDATE_SCHEMA: &str = r#"{
    "$id": "arcs.action_candidate.vertical_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["command"],
    "properties": {"command": {"type": "string", "minLength": 1}},
    "additionalProperties": false
}"#;

const RESULT_SCHEMA: &str = r#"{
    "$id": "arcs.execution_result.vertical_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["completed"],
    "properties": {"completed": {"type": "boolean"}},
    "additionalProperties": false
}"#;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-08-08T19:00:00Z".into()
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
                required_capabilities: vec![act_capability()],
                referenced_versions: request
                    .context
                    .iter()
                    .map(|item| item.version_id.clone())
                    .collect(),
                payload: json!({"command": "open door"}),
            }],
            trace: ReasoningTrace {
                model_name: "vertical-test-reasoner".into(),
                prompt_hash: "prompt-hash".into(),
                raw_output_hash: "result-hash".into(),
                temperature: 0.0,
            },
        })
    }
}

struct RecordingActAdapter {
    manifest: AdapterManifest,
    calls: Arc<Mutex<Vec<ActInvocation>>>,
}

impl ActAdapter for RecordingActAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn execute(&self, invocation: &ActInvocation) -> Result<ActResponse, AdapterCallError> {
        self.calls.lock().unwrap().push(invocation.clone());
        Ok(ActResponse {
            invocation_id: invocation.invocation_id.clone(),
            external_reference: "door-controller://operation-1".into(),
            result_payload: json!({"completed": true}),
        })
    }
}

struct FixedVerifier(VerificationVerdict);

impl Verifier for FixedVerifier {
    fn verify(&self, _artifact: &Artifact) -> Result<VerificationResult, VerificationError> {
        let detail = match self.0 {
            VerificationVerdict::Pass => "all checks passed",
            VerificationVerdict::Fail => "a required check failed",
            VerificationVerdict::Unknown => "available evidence is inconclusive",
        };
        Ok(VerificationResult {
            verdict: self.0,
            findings: vec![VerificationFinding {
                check: "execution_safety".into(),
                verdict: self.0,
                detail: detail.into(),
            }],
        })
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(CANDIDATE_SCHEMA).unwrap();
    schemas.register_json(RESULT_SCHEMA).unwrap();
    schemas
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
            max_candidates: 1,
        }),
    }
}

fn act_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId(ACT_ADAPTER.into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId(ACT_CAPABILITY.into()),
            contract: CapabilityContract::Act {
                accepts: vec![SchemaId(CANDIDATE_SCHEMA_ID.into())],
                emits: vec![SchemaId(RESULT_SCHEMA_ID.into())],
                idempotent: true,
            },
            required_permissions: vec!["door.write".into()],
        }],
    }
}

fn act_grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId(ACT_ADAPTER.into()),
        producer_class: ProducerClass::Executor,
        enabled_capabilities: vec![CapabilityId(ACT_CAPABILITY.into())],
        granted_permissions: vec!["door.write".into()],
        assigned_trust: TrustLevel::High,
        ingress_source_kind: None,
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        reasoning_limits: None,
    }
}

fn registry(
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
) -> (AdapterRegistry, AdapterManifest, AdapterManifest) {
    let reasoner_manifest = reasoning_manifest();
    let act_manifest = act_manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(input_manifest(), input_grant(), schemas, store)
        .unwrap();
    registry
        .register(reasoner_manifest.clone(), reasoning_grant(), schemas, store)
        .unwrap();
    registry
        .register(act_manifest.clone(), act_grant(), schemas, store)
        .unwrap();
    (registry, reasoner_manifest, act_manifest)
}

fn act_capability() -> CapabilityRef {
    CapabilityRef::new(ACT_ADAPTER, ACT_CAPABILITY)
}

fn input_message() -> InputMessage {
    InputMessage {
        capability_id: CapabilityId(INPUT_CAPABILITY.into()),
        external_subject: Some("conversation-vertical".into()),
        external_reference: "chat://conversation-vertical/message-1".into(),
        payload: json!({"raw_text": "open the door"}),
    }
}

fn reasoning_request(input: &Artifact) -> crate::reasoning::ReasoningRequest {
    crate::reasoning::ReasoningRequest {
        request_id: "vertical-cycle-1".into(),
        reasoning_capability: CapabilityRef::new(REASONING_ADAPTER, REASONING_CAPABILITY),
        objective: "derive a safe action candidate".into(),
        context: vec![ContextSelection {
            version_id: input.version_id.clone(),
            payload_fields: vec!["raw_text".into()],
        }],
        target_schema_id: SchemaId(CANDIDATE_SCHEMA_ID.into()),
        allowed_capabilities: vec![act_capability()],
        constraints: json!({"require_approval": true}),
        budget: ReasoningBudget {
            max_context_items: 1,
            max_context_bytes: 4096,
            max_output_tokens: 64,
            max_output_bytes: 4096,
            max_candidates: 1,
        },
    }
}

fn route_policy() -> KnownRoutePolicy {
    KnownRoutePolicy {
        eligible_schema_ids: vec![SchemaId(CANDIDATE_SCHEMA_ID.into())],
        minimum_trust: TrustLevel::Low,
    }
}

fn resolve_unknown_input(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    ids: &mut dyn ArtifactIdGenerator,
    reasoner: &dyn ReasoningAdapter,
) -> (Artifact, Artifact) {
    let input = InputService::new(registry, schemas, store, ids, &FixedClock)
        .ingest(&AdapterId(INPUT_ADAPTER.into()), input_message())
        .unwrap();
    let source = ActiveSource {
        version_id: input.version_id.clone(),
        activation: 1.0,
    };
    let mut reasoning = ReasoningService::new(registry, schemas, store, ids, &FixedClock, reasoner);
    let resolution = AgentCycle::new(store)
        .resolve_with_fallback(
            &mut reasoning,
            &[source],
            0.5,
            &route_policy(),
            reasoning_request(&input),
        )
        .unwrap();
    let RouteResolution::ReasonedCandidates(mut proposals) = resolution else {
        panic!("unknown input must use reasoning fallback");
    };
    assert_eq!(proposals.len(), 1);
    let candidate = AgentCycle::new(store)
        .commit_proposal(&mut reasoning, proposals.remove(0))
        .unwrap();
    (input, candidate)
}

fn verify_relation(
    store: &SqliteArtifactStore,
    from: &VersionId,
    to: &VersionId,
    kind: RelationKind,
) {
    assert!(
        store
            .outgoing_relations(from)
            .unwrap()
            .iter()
            .any(|relation| relation.to == *to && relation.kind == kind)
    );
}

struct CompletedCycle {
    input: Artifact,
    candidate: Artifact,
    verification: Artifact,
    approval: Artifact,
    action: Artifact,
    result: Artifact,
}

fn complete_cycle(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    ids: &mut dyn ArtifactIdGenerator,
    reasoner: &dyn ReasoningAdapter,
    act: &dyn ActAdapter,
) -> CompletedCycle {
    let (input, candidate) = resolve_unknown_input(registry, schemas, store, ids, reasoner);
    let verification = VerificationService::new(schemas, store, ids, &FixedClock)
        .verify(
            &candidate.version_id,
            &FixedVerifier(VerificationVerdict::Pass),
        )
        .unwrap();
    let approval = ApprovalService::new(schemas, store, ids, &FixedClock)
        .decide(
            &candidate.version_id,
            &verification.version_id,
            ApprovalDecision::Approved,
            Actor {
                actor_type: ActorType::Human,
                id: "operator.vertical-test".into(),
            },
            "all required checks passed".into(),
        )
        .unwrap();
    let action = ActionService::new(registry, schemas, store, ids, &FixedClock)
        .materialize(&approval.version_id, &act_capability())
        .unwrap();
    let result = ExecutionService::new(registry, schemas, store, ids, &FixedClock)
        .execute(act, &action.version_id)
        .unwrap();
    CompletedCycle {
        input,
        candidate,
        verification,
        approval,
        action,
        result,
    }
}

#[test]
fn unknown_input_can_complete_full_authorized_execution_cycle() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let (registry, reasoner_manifest, act_manifest) = registry(&schemas, &store);
    let reasoning_calls = Arc::new(Mutex::new(Vec::new()));
    let reasoner = RecordingReasoner {
        manifest: reasoner_manifest,
        calls: reasoning_calls.clone(),
    };
    let act_calls = Arc::new(Mutex::new(Vec::new()));
    let act = RecordingActAdapter {
        manifest: act_manifest,
        calls: act_calls.clone(),
    };
    let mut ids = TestIds(1);

    let completed = complete_cycle(&registry, &schemas, &store, &mut ids, &reasoner, &act);

    for artifact in [
        &completed.input,
        &completed.candidate,
        &completed.verification,
        &completed.approval,
        &completed.action,
        &completed.result,
    ] {
        assert_eq!(
            store.get(&artifact.version_id).unwrap().as_ref(),
            Some(artifact)
        );
    }
    assert_eq!(reasoning_calls.lock().unwrap().len(), 1);
    assert_eq!(act_calls.lock().unwrap().len(), 1);
    verify_relation(
        &store,
        &completed.verification.version_id,
        &completed.candidate.version_id,
        relation_kinds::verifies(),
    );
    verify_relation(
        &store,
        &completed.approval.version_id,
        &completed.candidate.version_id,
        relation_kinds::approves(),
    );
    verify_relation(
        &store,
        &completed.action.version_id,
        &completed.candidate.version_id,
        relation_kinds::derived_from(),
    );
    verify_relation(
        &store,
        &completed.action.version_id,
        &completed.approval.version_id,
        relation_kinds::based_on(),
    );
    verify_relation(
        &store,
        &completed.result.version_id,
        &completed.action.version_id,
        relation_kinds::result_of(),
    );
}

fn assert_verification_blocks_execution(verdict: VerificationVerdict) {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let (registry, reasoner_manifest, act_manifest) = registry(&schemas, &store);
    let reasoner = RecordingReasoner {
        manifest: reasoner_manifest,
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let act_calls = Arc::new(Mutex::new(Vec::new()));
    let _act = RecordingActAdapter {
        manifest: act_manifest,
        calls: act_calls.clone(),
    };
    let mut ids = TestIds(1);
    let (_, candidate) = resolve_unknown_input(&registry, &schemas, &store, &mut ids, &reasoner);
    let verification = VerificationService::new(&schemas, &store, &mut ids, &FixedClock)
        .verify(&candidate.version_id, &FixedVerifier(verdict))
        .unwrap();
    let artifacts_before_approval = store.len().unwrap();
    let next_artifact_id_before_approval = ids.0;

    let approval = ApprovalService::new(&schemas, &store, &mut ids, &FixedClock).decide(
        &candidate.version_id,
        &verification.version_id,
        ApprovalDecision::Approved,
        Actor {
            actor_type: ActorType::Human,
            id: "operator.vertical-test".into(),
        },
        "attempted approval".into(),
    );

    match verdict {
        VerificationVerdict::Fail => assert!(matches!(
            approval,
            Err(ApprovalError::CannotApproveFailedVerification)
        )),
        VerificationVerdict::Unknown => assert!(matches!(
            approval,
            Err(ApprovalError::CannotApproveUnknownVerification)
        )),
        VerificationVerdict::Pass => unreachable!(),
    }
    assert_eq!(store.len().unwrap(), artifacts_before_approval);
    assert_eq!(ids.0, next_artifact_id_before_approval);
    assert!(
        store
            .find_by_stream_key(&format!("approval:{}", candidate.version_id.0))
            .unwrap()
            .is_none()
    );
    assert!(act_calls.lock().unwrap().is_empty());
}

#[test]
fn failed_verification_never_reaches_execution() {
    assert_verification_blocks_execution(VerificationVerdict::Fail);
}

#[test]
fn unknown_verification_never_reaches_execution() {
    assert_verification_blocks_execution(VerificationVerdict::Unknown);
}

struct TemporaryDatabase {
    path: PathBuf,
}

impl TemporaryDatabase {
    fn new() -> Self {
        static NEXT_DATABASE: AtomicU64 = AtomicU64::new(1);
        let sequence = NEXT_DATABASE.fetch_add(1, Ordering::Relaxed);
        Self {
            path: std::env::temp_dir().join(format!(
                "arcs-full-cycle-{}-{sequence}.sqlite",
                std::process::id()
            )),
        }
    }
}

impl Drop for TemporaryDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn completed_execution_is_not_repeated_after_restart() {
    let schemas = schemas();
    let database = TemporaryDatabase::new();
    let reasoning_calls = Arc::new(Mutex::new(Vec::new()));
    let act_calls = Arc::new(Mutex::new(Vec::new()));
    let (registry, reasoner_manifest, act_manifest) = {
        let store = SqliteArtifactStore::open(&database.path).unwrap();
        registry(&schemas, &store)
    };
    let reasoner = RecordingReasoner {
        manifest: reasoner_manifest,
        calls: reasoning_calls.clone(),
    };
    let act = RecordingActAdapter {
        manifest: act_manifest,
        calls: act_calls.clone(),
    };

    let first = {
        let store = SqliteArtifactStore::open(&database.path).unwrap();
        let mut ids = TestIds(1);
        complete_cycle(&registry, &schemas, &store, &mut ids, &reasoner, &act)
    };
    let replay = {
        let store = SqliteArtifactStore::open(&database.path).unwrap();
        let mut ids = TestIds(1000);
        let (input, candidate) =
            resolve_unknown_input(&registry, &schemas, &store, &mut ids, &reasoner);
        assert_eq!(input, first.input);
        assert_eq!(candidate, first.candidate);
        let verification = store
            .find_by_stream_key(&format!("verification:{}", candidate.version_id.0))
            .unwrap()
            .unwrap();
        let approval = store
            .find_by_stream_key(&format!("approval:{}", candidate.version_id.0))
            .unwrap()
            .unwrap();
        let action = store
            .find_by_stream_key(&format!(
                "action:{}:{}:{}",
                approval.version_id.0, ACT_ADAPTER, ACT_CAPABILITY
            ))
            .unwrap()
            .unwrap();
        assert_eq!(verification, first.verification);
        assert_eq!(approval, first.approval);
        assert_eq!(action, first.action);
        ExecutionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
            .execute(&act, &action.version_id)
            .unwrap()
    };

    assert_eq!(replay, first.result);
    assert_eq!(reasoning_calls.lock().unwrap().len(), 1);
    assert_eq!(act_calls.lock().unwrap().len(), 1);
}
