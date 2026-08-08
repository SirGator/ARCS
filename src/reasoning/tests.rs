use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
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
use crate::store::SqliteArtifactStore;

const CANDIDATE_SCHEMA: &str = r#"{
    "$id": "arcs.route_candidate.reasoning_slice_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["summary"],
    "properties": {"summary": {"type": "string", "minLength": 1}},
    "additionalProperties": false
}"#;

const RESULT_SCHEMA: &str = r#"{
    "$id": "arcs.result.reasoning_slice_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["ok"],
    "properties": {"ok": {"type": "boolean"}},
    "additionalProperties": false
}"#;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-08-01T12:00:00Z".into()
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
    context_version: VersionId,
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
        Ok(ReasoningResponse {
            invocation_id: request.invocation_id.clone(),
            request_id: request.request_id.clone(),
            candidates: vec![ProposalSubmission {
                schema_id: request.target_schema_id.clone(),
                required_capabilities: vec![CapabilityRef::new("action.test", "device.set")],
                referenced_versions: vec![self.context_version.clone()],
                payload: json!({"summary": "set the requested state"}),
            }],
            trace: ReasoningTrace {
                model_name: "mock-model".into(),
                prompt_hash: "prompt-sha256".into(),
                raw_output_hash: "output-sha256".into(),
                temperature: 0.0,
            },
        })
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(CANDIDATE_SCHEMA).unwrap();
    schemas.register_json(RESULT_SCHEMA).unwrap();
    schemas
}

fn reasoning_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("reasoning.test".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("reasoning.propose".into()),
            contract: CapabilityContract::Reason {
                emits: vec![SchemaId(
                    "arcs.route_candidate.reasoning_slice_test.v1".into(),
                )],
            },
            required_permissions: vec![],
        }],
    }
}

fn reasoning_grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("reasoning.test".into()),
        producer_class: ProducerClass::Model,
        enabled_capabilities: vec![CapabilityId("reasoning.propose".into())],
        granted_permissions: vec![],
        assigned_trust: TrustLevel::Low,
        ingress_source_kind: None,
        max_payload_bytes: 8192,
        max_external_reference_bytes: 512,
        reasoning_limits: Some(ReasoningLimits {
            max_context_items: 4,
            max_context_bytes: 8192,
            max_output_tokens: 512,
            max_output_bytes: 8192,
            max_candidates: 2,
        }),
    }
}

fn action_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("action.test".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("device.set".into()),
            contract: CapabilityContract::Act {
                accepts: vec![SchemaId(
                    "arcs.route_candidate.reasoning_slice_test.v1".into(),
                )],
                emits: vec![SchemaId("arcs.result.reasoning_slice_test.v1".into())],
                idempotent: true,
            },
            required_permissions: vec!["device.write".into()],
        }],
    }
}

fn action_grant() -> AdapterGrant {
    AdapterGrant {
        adapter_id: AdapterId("action.test".into()),
        producer_class: ProducerClass::Executor,
        enabled_capabilities: vec![CapabilityId("device.set".into())],
        granted_permissions: vec!["device.write".into()],
        assigned_trust: TrustLevel::Medium,
        ingress_source_kind: None,
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        reasoning_limits: None,
    }
}

fn context() -> Artifact {
    Artifact::new(
        "input-1",
        "input-1-v1",
        "input",
        "arcs.input.v1",
        "2026-08-01T11:59:00Z",
        Actor {
            actor_type: ActorType::Human,
            id: "user-1".into(),
        },
        Source {
            kind: SourceKind::Chat,
            reference: "conversation-1".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::Human,
        },
        "conversation:1",
        json!({"raw_text": "switch device on"}),
    )
}

#[test]
fn reasoning_slice_audits_request_and_commits_low_trust_candidate() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let context = context();
    store.append(&context, &schemas).unwrap();
    let manifest = reasoning_manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(manifest.clone(), reasoning_grant(), &schemas, &store)
        .unwrap();
    registry
        .register(action_manifest(), action_grant(), &schemas, &store)
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let endpoint = MockReasoner {
        manifest,
        context_version: context.version_id.clone(),
        calls: Arc::clone(&calls),
    };
    let mut ids = TestIds(1);
    let mut service = ReasoningService::new(
        &registry,
        &schemas,
        &store,
        &mut ids,
        &FixedClock,
        &endpoint,
    );
    let request = ReasoningRequest {
        request_id: "reasoning-1".into(),
        reasoning_capability: CapabilityRef::new("reasoning.test", "reasoning.propose"),
        objective: "find a safe next step".into(),
        context: vec![ContextSelection {
            version_id: context.version_id.clone(),
            payload_fields: vec!["raw_text".into()],
        }],
        target_schema_id: SchemaId("arcs.route_candidate.reasoning_slice_test.v1".into()),
        allowed_capabilities: vec![CapabilityRef::new("action.test", "device.set")],
        constraints: json!({}),
        budget: ReasoningBudget {
            max_context_items: 2,
            max_context_bytes: 4096,
            max_output_tokens: 256,
            max_output_bytes: 4096,
            max_candidates: 1,
        },
    };

    let proposals = service.reason(request.clone()).unwrap();
    let replay = service.reason(request).unwrap();
    let proposal = proposals.into_iter().next().unwrap();
    let candidate = service.commit_proposal(proposal.clone()).unwrap();
    let duplicate = service.commit_proposal(proposal).unwrap();

    assert_eq!(replay.len(), 1);
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(duplicate, candidate);
    assert_eq!(candidate.created_by.actor_type, ActorType::Model);
    assert_eq!(candidate.trust.level, TrustLevel::Low);
    assert_eq!(candidate.trust.source_class, SourceClass::Model);
    assert!(store.len().unwrap() >= 6);
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
                "arcs-reasoning-invocation-{}-{sequence}.sqlite",
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

fn request_for(context: &Artifact) -> ReasoningRequest {
    ReasoningRequest {
        request_id: "reasoning-restart-1".into(),
        reasoning_capability: CapabilityRef::new("reasoning.test", "reasoning.propose"),
        objective: "find a safe next step".into(),
        context: vec![ContextSelection {
            version_id: context.version_id.clone(),
            payload_fields: vec!["raw_text".into()],
        }],
        target_schema_id: SchemaId("arcs.route_candidate.reasoning_slice_test.v1".into()),
        allowed_capabilities: vec![CapabilityRef::new("action.test", "device.set")],
        constraints: json!({}),
        budget: ReasoningBudget {
            max_context_items: 2,
            max_context_bytes: 4096,
            max_output_tokens: 256,
            max_output_bytes: 4096,
            max_candidates: 1,
        },
    }
}

#[test]
fn successful_reasoning_is_not_dispatched_again_after_restart() {
    let schemas = schemas();
    let database = TemporaryDatabase::new();
    let context = context();
    let manifest = reasoning_manifest();
    let mut registry = AdapterRegistry::new();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let endpoint = MockReasoner {
        manifest: manifest.clone(),
        context_version: context.version_id.clone(),
        calls: Arc::clone(&calls),
    };

    let first = {
        let store = SqliteArtifactStore::open(&database.path).unwrap();
        store.append(&context, &schemas).unwrap();
        registry
            .register(manifest, reasoning_grant(), &schemas, &store)
            .unwrap();
        registry
            .register(action_manifest(), action_grant(), &schemas, &store)
            .unwrap();
        let mut ids = TestIds(1);
        ReasoningService::new(
            &registry,
            &schemas,
            &store,
            &mut ids,
            &FixedClock,
            &endpoint,
        )
        .reason(request_for(&context))
        .unwrap()
    };
    let replay = {
        let store = SqliteArtifactStore::open(&database.path).unwrap();
        let mut ids = TestIds(99);
        ReasoningService::new(
            &registry,
            &schemas,
            &store,
            &mut ids,
            &FixedClock,
            &endpoint,
        )
        .reason(request_for(&context))
        .unwrap()
    };

    assert_eq!(replay, first);
    assert_eq!(calls.lock().unwrap().len(), 1);
}

fn assert_changed_reasoning_input_causes_identity_conflict(
    change: impl FnOnce(&mut ReasoningRequest),
) {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let context = context();
    store.append(&context, &schemas).unwrap();
    let manifest = reasoning_manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(manifest.clone(), reasoning_grant(), &schemas, &store)
        .unwrap();
    registry
        .register(action_manifest(), action_grant(), &schemas, &store)
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let endpoint = MockReasoner {
        manifest,
        context_version: context.version_id.clone(),
        calls: Arc::clone(&calls),
    };
    let mut ids = TestIds(1);
    let mut service = ReasoningService::new(
        &registry,
        &schemas,
        &store,
        &mut ids,
        &FixedClock,
        &endpoint,
    );

    service.reason(request_for(&context)).unwrap();
    let mut changed = request_for(&context);
    change(&mut changed);
    let result = service.reason(changed);

    assert!(matches!(
        result,
        Err(ReasoningError::Invocation(
            crate::runtime::InvocationError::IdentityConflict(_)
        ))
    ));
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[test]
fn reasoning_rejects_a_reused_request_id_with_a_different_objective() {
    assert_changed_reasoning_input_causes_identity_conflict(|changed| {
        changed.objective = "completely different objective".into();
    });
}

#[test]
fn reasoning_rejects_a_reused_request_id_with_different_constraints() {
    assert_changed_reasoning_input_causes_identity_conflict(|changed| {
        changed.constraints = json!({"do_not_enter": "room-4"});
    });
}
