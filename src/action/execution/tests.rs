use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::{ExecutionError, ExecutionService};
use crate::action::ActionService;
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, ActAdapter, ActInvocation, ActResponse, AdapterCallError,
    AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry, CapabilityContract,
    CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use crate::approval::{ApprovalDecision, ApprovalService};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds,
    SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId,
};
use crate::runtime::{
    InvocationKind, InvocationService, InvocationSpec, InvocationStatus,
    deterministic_input_fingerprint, deterministic_invocation_id,
};
use crate::store::{ArtifactNetwork, SqliteArtifactStore, relation_kinds};
use crate::verification::{
    VerificationError, VerificationFinding, VerificationResult, VerificationService,
    VerificationVerdict, Verifier,
};

const CANDIDATE_SCHEMA: &str = r#"{
    "$id": "arcs.route_candidate.execution_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["command"],
    "properties": {"command": {"type": "string", "minLength": 1}},
    "additionalProperties": false
}"#;

const RESULT_SCHEMA: &str = r#"{
    "$id": "arcs.execution_result.execution_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["completed"],
    "properties": {"completed": {"type": "boolean"}},
    "additionalProperties": false
}"#;

const SECOND_RESULT_SCHEMA: &str = r#"{
    "$id": "arcs.execution_result.alternate_execution_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["completed"],
    "properties": {"completed": {"type": "boolean"}},
    "additionalProperties": false
}"#;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-08-08T18:00:00Z".into()
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

struct PassVerifier;

impl Verifier for PassVerifier {
    fn verify(&self, _artifact: &Artifact) -> Result<VerificationResult, VerificationError> {
        Ok(VerificationResult {
            verdict: VerificationVerdict::Pass,
            findings: vec![VerificationFinding {
                check: "execution_safety".into(),
                verdict: VerificationVerdict::Pass,
                detail: "candidate is safe to execute".into(),
            }],
        })
    }
}

struct MockActAdapter {
    manifest: AdapterManifest,
    calls: Arc<Mutex<Vec<ActInvocation>>>,
    response_invocation_id: Option<String>,
}

impl ActAdapter for MockActAdapter {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn execute(&self, invocation: &ActInvocation) -> Result<ActResponse, AdapterCallError> {
        self.calls.lock().unwrap().push(invocation.clone());
        Ok(ActResponse {
            invocation_id: self
                .response_invocation_id
                .clone()
                .unwrap_or_else(|| invocation.invocation_id.clone()),
            external_reference: "device-operation-1".into(),
            result_payload: json!({"completed": true}),
        })
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(CANDIDATE_SCHEMA).unwrap();
    schemas.register_json(RESULT_SCHEMA).unwrap();
    schemas.register_json(SECOND_RESULT_SCHEMA).unwrap();
    schemas
}

fn act_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("action.test".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![
            CapabilityDescriptor {
                id: CapabilityId("device.set".into()),
                contract: CapabilityContract::Act {
                    accepts: vec![SchemaId("arcs.route_candidate.execution_test.v1".into())],
                    emits: vec![SchemaId("arcs.execution_result.execution_test.v1".into())],
                    idempotent: true,
                },
                required_permissions: vec!["device.write".into()],
            },
            CapabilityDescriptor {
                id: CapabilityId("device.transform".into()),
                contract: CapabilityContract::Transform {
                    accepts: vec![SchemaId("arcs.route_candidate.execution_test.v1".into())],
                    emits: vec![SchemaId("arcs.execution_result.execution_test.v1".into())],
                },
                required_permissions: vec![],
            },
        ],
    }
}

fn act_grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("action.test".into()),
        producer_class: ProducerClass::Executor,
        enabled_capabilities: vec![
            CapabilityId("device.set".into()),
            CapabilityId("device.transform".into()),
        ],
        granted_permissions: vec!["device.write".into()],
        assigned_trust: TrustLevel::High,
        ingress_source_kind: None,
        max_payload_bytes: 4096,
        max_external_reference_bytes: 256,
        reasoning_limits: None,
    }
}

fn registry(
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
) -> (AdapterRegistry, AdapterManifest) {
    let manifest = act_manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(manifest.clone(), act_grant(), schemas, store)
        .unwrap();
    (registry, manifest)
}

fn selected_capability() -> CapabilityRef {
    CapabilityRef::new("action.test", "device.set")
}

fn candidate() -> Artifact {
    let capability = selected_capability();
    let mut candidate = Artifact::new(
        "candidate-1",
        "candidate-1-v1",
        "route_candidate",
        "arcs.route_candidate.execution_test.v1",
        "2026-08-08T17:59:00Z",
        Actor {
            actor_type: ActorType::Model,
            id: "llm.test".into(),
        },
        Source {
            kind: SourceKind::External,
            reference: "reasoning:execution-test".into(),
        },
        Trust {
            level: TrustLevel::Low,
            source_class: SourceClass::Model,
        },
        "reasoning:execution-test",
        json!({"command": "open door"}),
    );
    candidate.tags = vec![format!(
        "requires:{}:{}",
        capability.adapter_id.0, capability.capability_id.0
    )];
    candidate
}

fn approval_chain(
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    ids: &mut dyn ArtifactIdGenerator,
    candidate: &Artifact,
    decision: ApprovalDecision,
) -> Artifact {
    store.append(candidate, schemas).unwrap();
    let report = VerificationService::new(schemas, store, ids, &FixedClock)
        .verify(&candidate.version_id, &PassVerifier)
        .unwrap();
    ApprovalService::new(schemas, store, ids, &FixedClock)
        .decide(
            &candidate.version_id,
            &report.version_id,
            decision,
            Actor {
                actor_type: ActorType::Human,
                id: "operator.test".into(),
            },
            "operator decision".into(),
        )
        .unwrap()
}

fn materialize(
    registry: &AdapterRegistry,
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    ids: &mut dyn ArtifactIdGenerator,
    approval: &Artifact,
) -> Artifact {
    ActionService::new(registry, schemas, store, ids, &FixedClock)
        .materialize(&approval.version_id, &selected_capability())
        .unwrap()
}

fn forged_action(
    candidate: &Artifact,
    approval: &Artifact,
    capability: &CapabilityRef,
    payload: Value,
) -> Artifact {
    Artifact::new(
        "action-forged",
        "action-forged-v1",
        "action",
        "arcs.action.v1",
        "2026-08-08T18:00:00Z",
        Actor {
            actor_type: ActorType::System,
            id: "arcs.action".into(),
        },
        Source {
            kind: SourceKind::Internal,
            reference: format!("action:{}", approval.version_id.0),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        format!("action:{}", approval.version_id.0),
        json!({
            "target_version": candidate.version_id.0,
            "approval_version": approval.version_id.0,
            "capability": {
                "adapter_id": capability.adapter_id.0,
                "capability_id": capability.capability_id.0
            },
            "input_schema_id": candidate.schema_id.0,
            "payload": payload
        }),
    )
}

fn endpoint(manifest: AdapterManifest) -> (MockActAdapter, Arc<Mutex<Vec<ActInvocation>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    (
        MockActAdapter {
            manifest,
            calls: calls.clone(),
            response_invocation_id: None,
        },
        calls,
    )
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
                "arcs-execution-invocation-{}-{sequence}.sqlite",
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
fn approved_action_is_executed_and_result_is_persisted() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let (registry, manifest) = registry(&schemas, &store);
    let candidate = candidate();
    let mut ids = TestIds(1);
    let approval = approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Approved,
    );
    let action = materialize(&registry, &schemas, &store, &mut ids, &approval);
    let (endpoint, calls) = endpoint(manifest);

    let result = ExecutionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .execute(&endpoint, &action.version_id)
        .unwrap();

    assert_eq!(calls.lock().unwrap().len(), 1);
    let invocation = calls.lock().unwrap()[0].clone();
    assert_eq!(invocation.action_version_id, action.version_id);
    assert_eq!(invocation.action_schema_id.0, "arcs.action.v1");
    assert_eq!(invocation.payload, action.payload);
    assert_eq!(
        invocation.result_schema_id.0,
        "arcs.execution_result.execution_test.v1"
    );
    assert_eq!(result.artifact_type, "execution_result");
    assert_eq!(result.created_by.actor_type, ActorType::Executor);
    assert_eq!(result.payload, json!({"completed": true}));
    assert_eq!(store.get(&result.version_id).unwrap(), Some(result.clone()));

    let relations = store.outgoing_relations(&result.version_id).unwrap();
    assert!(relations.iter().any(|relation| {
        relation.to == action.version_id && relation.kind == relation_kinds::result_of()
    }));
    assert_eq!(relations.len(), 1);
    assert!(
        ArtifactNetwork::new(&store)
            .neighbors(&result.version_id)
            .unwrap()
            .is_empty()
    );
    let state = InvocationService::new(&store, &schemas, &FixedClock)
        .lookup(&invocation.invocation_id)
        .unwrap()
        .unwrap();
    assert_eq!(state.status, InvocationStatus::Succeeded);
    assert_eq!(state.input_version, action.version_id);
    assert_eq!(state.result_version, Some(result.version_id));
}

#[test]
fn candidate_cannot_be_executed_directly() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let (registry, manifest) = registry(&schemas, &store);
    let candidate = candidate();
    store.append(&candidate, &schemas).unwrap();
    let (endpoint, calls) = endpoint(manifest);
    let mut ids = TestIds(1);

    let result = ExecutionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .execute(&endpoint, &candidate.version_id);

    assert!(matches!(result, Err(ExecutionError::NotActionArtifact)));
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn approval_cannot_be_executed_without_materialized_action() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let (registry, manifest) = registry(&schemas, &store);
    let candidate = candidate();
    let mut ids = TestIds(1);
    let approval = approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Approved,
    );
    let (endpoint, calls) = endpoint(manifest);

    let result = ExecutionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .execute(&endpoint, &approval.version_id);

    assert!(matches!(result, Err(ExecutionError::NotActionArtifact)));
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn action_without_materialization_relations_cannot_execute() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let (registry, manifest) = registry(&schemas, &store);
    let candidate = candidate();
    let mut ids = TestIds(1);
    let approval = approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Approved,
    );
    let action = forged_action(
        &candidate,
        &approval,
        &selected_capability(),
        candidate.payload.clone(),
    );
    store.append(&action, &schemas).unwrap();
    let (endpoint, calls) = endpoint(manifest);

    let result = ExecutionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .execute(&endpoint, &action.version_id);

    assert!(matches!(
        result,
        Err(ExecutionError::MissingActionTargetRelation)
    ));
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn changed_payload_in_action_cannot_execute() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let (registry, manifest) = registry(&schemas, &store);
    let candidate = candidate();
    let mut ids = TestIds(1);
    let approval = approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Approved,
    );
    let action = forged_action(
        &candidate,
        &approval,
        &selected_capability(),
        json!({"command": "delete file"}),
    );
    store
        .append_related(
            &action,
            &schemas,
            &[
                (candidate.version_id.clone(), relation_kinds::derived_from()),
                (approval.version_id.clone(), relation_kinds::based_on()),
            ],
        )
        .unwrap();
    let (endpoint, calls) = endpoint(manifest);

    let result = ExecutionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .execute(&endpoint, &action.version_id);

    assert!(matches!(result, Err(ExecutionError::ActionPayloadMismatch)));
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn non_act_capability_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let (registry, manifest) = registry(&schemas, &store);
    let candidate = candidate();
    let mut ids = TestIds(1);
    let approval = approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Approved,
    );
    let transform = CapabilityRef::new("action.test", "device.transform");
    let action = forged_action(&candidate, &approval, &transform, candidate.payload.clone());
    store
        .append_related(
            &action,
            &schemas,
            &[
                (candidate.version_id.clone(), relation_kinds::derived_from()),
                (approval.version_id.clone(), relation_kinds::based_on()),
            ],
        )
        .unwrap();
    let (endpoint, calls) = endpoint(manifest);

    let result = ExecutionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .execute(&endpoint, &action.version_id);

    assert!(matches!(
        result,
        Err(ExecutionError::CapabilityIsNotAct(capability)) if capability == transform
    ));
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn wrong_adapter_cannot_execute_action() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let (registry, _) = registry(&schemas, &store);
    let candidate = candidate();
    let mut ids = TestIds(1);
    let approval = approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Approved,
    );
    let action = materialize(&registry, &schemas, &store, &mut ids, &approval);
    let mut wrong_manifest = act_manifest();
    wrong_manifest.adapter_id = AdapterId("other.executor".into());
    let (endpoint, calls) = endpoint(wrong_manifest);

    let result = ExecutionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .execute(&endpoint, &action.version_id);

    assert!(matches!(
        result,
        Err(ExecutionError::EndpointAdapterMismatch)
    ));
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn act_capability_must_emit_exactly_one_result_schema() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut manifest = act_manifest();
    let CapabilityContract::Act { emits, .. } = &mut manifest.capabilities[0].contract else {
        unreachable!();
    };
    emits.push(SchemaId(
        "arcs.execution_result.alternate_execution_test.v1".into(),
    ));
    let mut registry = AdapterRegistry::new();
    registry
        .register(manifest.clone(), act_grant(), &schemas, &store)
        .unwrap();
    let candidate = candidate();
    let mut ids = TestIds(1);
    let approval = approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Approved,
    );
    let action = materialize(&registry, &schemas, &store, &mut ids, &approval);
    let (endpoint, calls) = endpoint(manifest);

    let result = ExecutionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .execute(&endpoint, &action.version_id);

    assert!(matches!(
        result,
        Err(ExecutionError::InvalidResultSchemaCount { actual: 2 })
    ));
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn response_with_wrong_invocation_id_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let (registry, manifest) = registry(&schemas, &store);
    let candidate = candidate();
    let mut ids = TestIds(1);
    let approval = approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Approved,
    );
    let action = materialize(&registry, &schemas, &store, &mut ids, &approval);
    let (mut endpoint, calls) = endpoint(manifest);
    endpoint.response_invocation_id = Some("wrong-invocation".into());

    let result = ExecutionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .execute(&endpoint, &action.version_id);

    assert!(matches!(
        result,
        Err(ExecutionError::InvocationResponseMismatch)
    ));
    assert_eq!(calls.lock().unwrap().len(), 1);
    let invocation_id = calls.lock().unwrap()[0].invocation_id.clone();
    let state = InvocationService::new(&store, &schemas, &FixedClock)
        .lookup(&invocation_id)
        .unwrap()
        .unwrap();
    assert_eq!(state.status, InvocationStatus::Failed);
    assert!(state.result_version.is_none());
}

#[test]
fn successful_execution_is_not_dispatched_again_after_restart() {
    let schemas = schemas();
    let database = TemporaryDatabase::new();
    let manifest = act_manifest();
    let mut registry = AdapterRegistry::new();
    let (endpoint, calls) = endpoint(manifest.clone());
    let (first, action_version) = {
        let store = SqliteArtifactStore::open(&database.path).unwrap();
        registry
            .register(manifest, act_grant(), &schemas, &store)
            .unwrap();
        let candidate = candidate();
        let mut ids = TestIds(1);
        let approval = approval_chain(
            &schemas,
            &store,
            &mut ids,
            &candidate,
            ApprovalDecision::Approved,
        );
        let action = materialize(&registry, &schemas, &store, &mut ids, &approval);
        let result = ExecutionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
            .execute(&endpoint, &action.version_id)
            .unwrap();
        (result, action.version_id)
    };
    let replay = {
        let store = SqliteArtifactStore::open(&database.path).unwrap();
        let mut ids = TestIds(100);
        ExecutionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
            .execute(&endpoint, &action_version)
            .unwrap()
    };

    assert_eq!(replay, first);
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[test]
fn dispatched_execution_recovers_with_same_invocation_id() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let (registry, manifest) = registry(&schemas, &store);
    let candidate = candidate();
    let mut ids = TestIds(1);
    let approval = approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Approved,
    );
    let action = materialize(&registry, &schemas, &store, &mut ids, &approval);
    let capability = selected_capability();
    let result_schema = SchemaId("arcs.execution_result.execution_test.v1".into());
    let invocation_id = deterministic_invocation_id(
        InvocationKind::Execution,
        &[
            &action.version_id.0,
            &capability.adapter_id.0,
            &capability.capability_id.0,
            &result_schema.0,
        ],
    );
    let invocation = ActInvocation {
        invocation_id: invocation_id.clone(),
        capability,
        action_version_id: action.version_id.clone(),
        action_schema_id: action.schema_id.clone(),
        payload: action.payload.clone(),
        result_schema_id: result_schema,
    };
    let fingerprint =
        deterministic_input_fingerprint(&[&serde_json::to_string(&invocation).unwrap()]);
    let invocations = InvocationService::new(&store, &schemas, &FixedClock);
    let prepared = invocations
        .prepare(InvocationSpec {
            invocation_id: invocation_id.clone(),
            kind: InvocationKind::Execution,
            capability: "action.test/device.set".into(),
            input_version: action.version_id.clone(),
            input_fingerprint: fingerprint,
        })
        .unwrap();
    invocations.dispatch(&prepared).unwrap();
    let (endpoint, calls) = endpoint(manifest);

    ExecutionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .execute(&endpoint, &action.version_id)
        .unwrap();

    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(calls.lock().unwrap()[0].invocation_id, invocation_id);
    let state = InvocationService::new(&store, &schemas, &FixedClock)
        .lookup(&invocation_id)
        .unwrap()
        .unwrap();
    assert_eq!(state.status, InvocationStatus::Succeeded);
}
