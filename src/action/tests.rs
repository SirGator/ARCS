use serde_json::json;

use super::{ActionError, ActionService};
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry,
    CapabilityContract, CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use crate::approval::{ApprovalDecision, ApprovalService};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds,
    SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId,
};
use crate::store::{ArtifactNetwork, SqliteArtifactStore, relation_kinds};
use crate::verification::{
    VerificationError, VerificationFinding, VerificationResult, VerificationService,
    VerificationVerdict, Verifier,
};

const CANDIDATE_SCHEMA: &str = r#"{
    "$id": "arcs.route_candidate.action_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["command"],
    "properties": {"command": {"type": "string", "minLength": 1}},
    "additionalProperties": false
}"#;

const OTHER_INPUT_SCHEMA: &str = r#"{
    "$id": "arcs.other_input.action_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "properties": {},
    "additionalProperties": false
}"#;

const RESULT_SCHEMA: &str = r#"{
    "$id": "arcs.action_result.action_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["ok"],
    "properties": {"ok": {"type": "boolean"}},
    "additionalProperties": false
}"#;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-08-08T17:00:00Z".into()
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
                check: "action_safety".into(),
                verdict: VerificationVerdict::Pass,
                detail: "candidate may be materialized".into(),
            }],
        })
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(CANDIDATE_SCHEMA).unwrap();
    schemas.register_json(OTHER_INPUT_SCHEMA).unwrap();
    schemas.register_json(RESULT_SCHEMA).unwrap();
    schemas
}

fn manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("action.test".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![
            CapabilityDescriptor {
                id: CapabilityId("device.set".into()),
                contract: CapabilityContract::Act {
                    accepts: vec![SchemaId("arcs.route_candidate.action_test.v1".into())],
                    emits: vec![SchemaId("arcs.action_result.action_test.v1".into())],
                    idempotent: true,
                },
                required_permissions: vec!["device.write".into()],
            },
            CapabilityDescriptor {
                id: CapabilityId("device.incompatible".into()),
                contract: CapabilityContract::Act {
                    accepts: vec![SchemaId("arcs.other_input.action_test.v1".into())],
                    emits: vec![SchemaId("arcs.action_result.action_test.v1".into())],
                    idempotent: true,
                },
                required_permissions: vec!["device.write".into()],
            },
            CapabilityDescriptor {
                id: CapabilityId("device.transform".into()),
                contract: CapabilityContract::Transform {
                    accepts: vec![SchemaId("arcs.route_candidate.action_test.v1".into())],
                    emits: vec![SchemaId("arcs.action_result.action_test.v1".into())],
                },
                required_permissions: vec![],
            },
        ],
    }
}

fn registry(schemas: &SchemaRegistry, store: &SqliteArtifactStore) -> AdapterRegistry {
    let mut registry = AdapterRegistry::new();
    registry
        .register(
            manifest(),
            AdapterGrant {
                adapter_id: AdapterId("action.test".into()),
                producer_class: ProducerClass::Executor,
                enabled_capabilities: vec![
                    CapabilityId("device.set".into()),
                    CapabilityId("device.incompatible".into()),
                    CapabilityId("device.transform".into()),
                ],
                granted_permissions: vec!["device.write".into()],
                assigned_trust: TrustLevel::High,
                ingress_source_kind: None,
                max_payload_bytes: 4096,
                max_external_reference_bytes: 256,
                reasoning_limits: None,
            },
            schemas,
            store,
        )
        .unwrap();
    registry
}

fn capability(id: &str) -> CapabilityRef {
    CapabilityRef::new("action.test", id)
}

fn candidate(required: &CapabilityRef) -> Artifact {
    let mut candidate = Artifact::new(
        "candidate-1",
        "candidate-1-v1",
        "route_candidate",
        "arcs.route_candidate.action_test.v1",
        "2026-08-08T16:59:00Z",
        Actor {
            actor_type: ActorType::Model,
            id: "llm.test".into(),
        },
        Source {
            kind: SourceKind::External,
            reference: "reasoning:action-test".into(),
        },
        Trust {
            level: TrustLevel::Low,
            source_class: SourceClass::Model,
        },
        "reasoning:action-test",
        json!({"command": "open door"}),
    );
    candidate.tags = vec![format!(
        "requires:{}:{}",
        required.adapter_id.0, required.capability_id.0
    )];
    candidate
}

fn persist_approval_chain(
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

#[test]
fn approved_candidate_can_be_materialized_as_action() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, &store);
    let selected = capability("device.set");
    let candidate = candidate(&selected);
    let mut ids = TestIds(1);
    let approval = persist_approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Approved,
    );

    let action = ActionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .materialize(&approval.version_id, &selected)
        .unwrap();

    assert_eq!(action.artifact_type, "action");
    assert_eq!(action.created_by.actor_type, ActorType::System);
    assert_eq!(action.source.kind, SourceKind::Internal);
    assert_eq!(action.trust.level, TrustLevel::High);
    assert_eq!(
        action.payload,
        json!({
            "target_version": candidate.version_id.0,
            "approval_version": approval.version_id.0,
            "capability": {
                "adapter_id": "action.test",
                "capability_id": "device.set"
            },
            "input_schema_id": candidate.schema_id.0,
            "payload": candidate.payload
        })
    );
    assert_eq!(store.get(&action.version_id).unwrap(), Some(action.clone()));
    let relations = store.outgoing_relations(&action.version_id).unwrap();
    assert!(relations.iter().any(|relation| {
        relation.to == candidate.version_id && relation.kind == relation_kinds::derived_from()
    }));
    assert!(relations.iter().any(|relation| {
        relation.to == approval.version_id && relation.kind == relation_kinds::based_on()
    }));
    assert!(
        ArtifactNetwork::new(&store)
            .neighbors(&action.version_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rejected_candidate_cannot_be_materialized() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, &store);
    let selected = capability("device.set");
    let candidate = candidate(&selected);
    let mut ids = TestIds(1);
    let rejection = persist_approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Rejected,
    );

    let result = ActionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .materialize(&rejection.version_id, &selected);

    assert!(matches!(result, Err(ActionError::ApprovalNotApproved)));
    assert_eq!(store.len().unwrap(), 3);
}

#[test]
fn approval_without_approves_relation_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, &store);
    let selected = capability("device.set");
    let candidate = candidate(&selected);
    store.append(&candidate, &schemas).unwrap();
    let approval = Artifact::new(
        "approval-forged",
        "approval-forged-v1",
        "approval",
        "arcs.approval.v1",
        "2026-08-08T17:00:00Z",
        Actor {
            actor_type: ActorType::Human,
            id: "operator.test".into(),
        },
        Source {
            kind: SourceKind::Internal,
            reference: "approval:candidate-1-v1".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::Human,
        },
        "approval:candidate-1-v1",
        json!({
            "target_version": candidate.version_id.0,
            "verification_report_version": "verification-report-1-v1",
            "decision": "approved",
            "reason": "payload without relation"
        }),
    );
    store.append(&approval, &schemas).unwrap();
    let mut ids = TestIds(1);

    let result = ActionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .materialize(&approval.version_id, &selected);

    assert!(matches!(result, Err(ActionError::MissingApprovalRelation)));
    assert_eq!(store.len().unwrap(), 2);
}

#[test]
fn non_act_capability_cannot_be_materialized() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, &store);
    let selected = capability("device.transform");
    let candidate = candidate(&selected);
    let mut ids = TestIds(1);
    let approval = persist_approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Approved,
    );

    let result = ActionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .materialize(&approval.version_id, &selected);

    assert!(matches!(
        result,
        Err(ActionError::CapabilityIsNotAct(found)) if found == selected
    ));
}

#[test]
fn act_capability_that_does_not_accept_candidate_schema_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, &store);
    let selected = capability("device.incompatible");
    let candidate = candidate(&selected);
    let mut ids = TestIds(1);
    let approval = persist_approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Approved,
    );

    let result = ActionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .materialize(&approval.version_id, &selected);

    assert!(matches!(
        result,
        Err(ActionError::CandidateSchemaNotAccepted { capability, schema })
            if capability == selected && schema == candidate.schema_id
    ));
}

#[test]
fn capability_not_required_by_candidate_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, &store);
    let required = capability("device.transform");
    let selected = capability("device.set");
    let candidate = candidate(&required);
    let mut ids = TestIds(1);
    let approval = persist_approval_chain(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        ApprovalDecision::Approved,
    );

    let result = ActionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
        .materialize(&approval.version_id, &selected);

    assert!(matches!(
        result,
        Err(ActionError::CapabilityNotRequired(found)) if found == selected
    ));
}
