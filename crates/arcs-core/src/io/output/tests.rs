use std::collections::HashSet;
use std::sync::Mutex;

use super::*;
use crate::adapters::{
    AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry, CapabilityContract,
    CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use crate::approval::{ApprovalDecision, ApprovalService};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, SchemaId, SchemaRegistry, Source, SourceClass,
    SourceKind, SystemClock, Trust, TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, relation_kinds};
use crate::verification::{
    VerificationError, VerificationResult, VerificationService, VerificationVerdict, Verifier,
};
use serde_json::json;

const INPUT: &str = "arcs.report.test.v1";
const RESULT: &str = "arcs.delivery_receipt.test.v1";

struct Check(VerificationVerdict);
impl Verifier for Check {
    fn verify(&self, _: &Artifact) -> Result<VerificationResult, VerificationError> {
        Ok(VerificationResult {
            verdict: self.0,
            findings: vec![],
        })
    }
}
struct Endpoint {
    manifest: AdapterManifest,
    calls: Mutex<Vec<OutputInvocation>>,
    effects: Mutex<HashSet<String>>,
    failure: u8,
}
impl OutputAdapter for Endpoint {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }
    fn deliver(
        &self,
        invocation: &OutputInvocation,
    ) -> Result<OutputResponse, crate::adapters::AdapterCallError> {
        self.calls.lock().unwrap().push(invocation.clone());
        self.effects
            .lock()
            .unwrap()
            .insert(invocation.invocation_id.clone());
        if self.failure == 4 {
            return Err(crate::adapters::AdapterCallError::Timeout);
        }
        Ok(OutputResponse {
            invocation_id: if self.failure == 1 {
                "wrong".into()
            } else {
                invocation.invocation_id.clone()
            },
            external_reference: "output:stored".into(),
            result_payload: match self.failure {
                2 => json!({"status":"wrong"}),
                3 => json!({"status":"x".repeat(5000)}),
                _ => json!({"status":"delivered"}),
            },
        })
    }
}
struct Fixture {
    schemas: SchemaRegistry,
    store: SqliteArtifactStore,
    endpoint: Endpoint,
    grant: AdapterGrant,
}
impl Fixture {
    fn new() -> Self {
        let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
        schemas.register_json(&json!({"$id":INPUT,"type":"object","required":["body"],"properties":{"body":{"type":"string"}},"additionalProperties":false}).to_string()).unwrap();
        schemas.register_json(&json!({"$id":RESULT,"type":"object","required":["status"],"properties":{"status":{"type":"string","enum":["delivered"]}},"additionalProperties":false}).to_string()).unwrap();
        let descriptor = |name: &str| CapabilityDescriptor {
            id: CapabilityId(name.into()),
            contract: CapabilityContract::Output {
                accepts: vec![SchemaId(INPUT.into())],
                emits: vec![SchemaId(RESULT.into())],
                idempotent: true,
            },
            required_permissions: vec!["output.write".into()],
        };
        let manifest = AdapterManifest {
            protocol_version: 1,
            adapter_id: AdapterId("output.test".into()),
            adapter_version: "1".into(),
            capabilities: vec![descriptor("report.write"), descriptor("other.write")],
        };
        let grant = AdapterGrant {
            adapter_id: manifest.adapter_id.clone(),
            producer_class: ProducerClass::Executor,
            enabled_capabilities: manifest
                .capabilities
                .iter()
                .map(|cap| cap.id.clone())
                .collect(),
            granted_permissions: vec!["output.write".into()],
            assigned_trust: TrustLevel::Medium,
            ingress_source_kind: None,
            max_payload_bytes: 4096,
            max_external_reference_bytes: 256,
            reasoning_limits: None,
            data_limits: None,
        };
        let store = SqliteArtifactStore::in_memory().unwrap();
        let input = Artifact::new(
            "report",
            "report-v1",
            "report",
            INPUT,
            "2026-10-23T00:00:00Z",
            Actor {
                actor_type: ActorType::Adapter,
                id: "source".into(),
            },
            Source {
                kind: SourceKind::External,
                reference: "report:1".into(),
            },
            Trust {
                level: TrustLevel::Low,
                source_class: SourceClass::External,
            },
            "report:1",
            json!({"body":"Example"}),
        );
        store.append(&input, &schemas).unwrap();
        Self {
            schemas,
            store,
            endpoint: Endpoint {
                manifest,
                calls: Mutex::new(vec![]),
                effects: Mutex::new(HashSet::new()),
                failure: 0,
            },
            grant,
        }
    }
    fn registry(&self) -> AdapterRegistry {
        let mut registry = AdapterRegistry::new();
        registry
            .register(
                self.endpoint.manifest.clone(),
                self.grant.clone(),
                &self.schemas,
                &self.store,
            )
            .unwrap();
        registry
    }
    fn prepare(&self, cap: &str) -> Result<Artifact, OutputError> {
        OutputService::new(
            &self.registry(),
            &self.schemas,
            &self.store,
            &mut self.store.durable_id_generator("output-test").unwrap(),
            &SystemClock,
        )
        .prepare(
            &CapabilityRef::new("output.test", cap),
            &VersionId("report-v1".into()),
            &SchemaId(RESULT.into()),
        )
    }
    fn approve(&self, target: &VersionId, decision: ApprovalDecision) -> Artifact {
        let mut ids = self.store.durable_id_generator("approval-test").unwrap();
        let report = VerificationService::new(&self.schemas, &self.store, &mut ids, &SystemClock)
            .verify(target, &Check(VerificationVerdict::Pass))
            .unwrap();
        ApprovalService::new(&self.schemas, &self.store, &mut ids, &SystemClock)
            .decide(
                target,
                &report.version_id,
                decision,
                Actor {
                    actor_type: ActorType::System,
                    id: "test.policy".into(),
                },
                "explicit test policy".into(),
            )
            .unwrap()
    }
    fn deliver(&self, request: &VersionId, approval: &VersionId) -> Result<Artifact, OutputError> {
        OutputService::new(
            &self.registry(),
            &self.schemas,
            &self.store,
            &mut self.store.durable_id_generator("output-test").unwrap(),
            &SystemClock,
        )
        .deliver(&self.endpoint, request, approval)
    }
}

#[test]
fn output_requires_bound_request_and_commits_receipt_with_stable_replay() {
    let fixture = Fixture::new();
    let request = fixture.prepare("report.write").unwrap();
    assert_eq!(fixture.prepare("report.write").unwrap(), request);
    assert!(fixture.endpoint.calls.lock().unwrap().is_empty());
    let approval = fixture.approve(&request.version_id, ApprovalDecision::Approved);
    let result = fixture
        .deliver(&request.version_id, &approval.version_id)
        .unwrap();
    let count = fixture.store.len().unwrap();
    assert_eq!(result.payload, json!({"status":"delivered"}));
    assert_eq!(result.created_by.actor_type, ActorType::Executor);
    assert!(
        fixture
            .store
            .outgoing_relations(&result.version_id)
            .unwrap()
            .iter()
            .any(|relation| relation.to == request.version_id
                && relation.kind == relation_kinds::result_of())
    );
    assert_eq!(
        fixture
            .deliver(&request.version_id, &approval.version_id)
            .unwrap(),
        result
    );
    assert_eq!(fixture.store.len().unwrap(), count);
    assert_eq!(fixture.endpoint.calls.lock().unwrap().len(), 1);
}

#[test]
fn rejected_missing_or_another_target_approval_never_reaches_the_sink() {
    let fixture = Fixture::new();
    let request = fixture.prepare("report.write").unwrap();
    assert!(matches!(
        fixture.deliver(&request.version_id, &VersionId("missing".into())),
        Err(OutputError::MissingArtifact(_))
    ));
    let rejected = fixture.approve(&request.version_id, ApprovalDecision::Rejected);
    assert!(matches!(
        fixture.deliver(&request.version_id, &rejected.version_id),
        Err(OutputError::Gate(_))
    ));
    let other = fixture.prepare("other.write").unwrap();
    let approval = fixture.approve(&other.version_id, ApprovalDecision::Approved);
    assert!(matches!(
        fixture.deliver(&request.version_id, &approval.version_id),
        Err(OutputError::Gate(_))
    ));
    assert!(matches!(
        fixture.deliver(&VersionId("report-v1".into()), &approval.version_id),
        Err(OutputError::Gate(_))
    ));
    assert!(fixture.endpoint.calls.lock().unwrap().is_empty());
}

#[test]
fn forged_approval_and_missing_authority_relations_are_rejected() {
    let fixture = Fixture::new();
    let request = fixture.prepare("report.write").unwrap();
    let approval = fixture.approve(&request.version_id, ApprovalDecision::Approved);
    let mut fake = approval.clone();
    fake.artifact_id = ArtifactId("fake-approval".into());
    fake.version_id = VersionId("fake-approval-v1".into());
    fake.created_by.actor_type = ActorType::Model;
    fake.trust.source_class = SourceClass::Model;
    fixture.store.append(&fake, &fixture.schemas).unwrap();
    assert!(matches!(
        fixture.deliver(&request.version_id, &fake.version_id),
        Err(OutputError::Gate(_))
    ));
    let mut missing = approval;
    missing.artifact_id = ArtifactId("missing-relations".into());
    missing.version_id = VersionId("missing-relations-v1".into());
    fixture.store.append(&missing, &fixture.schemas).unwrap();
    assert!(matches!(
        fixture.deliver(&request.version_id, &missing.version_id),
        Err(OutputError::Gate(_))
    ));
    assert!(fixture.endpoint.calls.lock().unwrap().is_empty());
}

#[test]
fn timeouts_recover_with_the_same_id_without_a_second_effect() {
    let mut fixture = Fixture::new();
    let request = fixture.prepare("report.write").unwrap();
    let approval = fixture.approve(&request.version_id, ApprovalDecision::Approved);
    fixture.endpoint.failure = 4;
    assert!(matches!(
        fixture.deliver(&request.version_id, &approval.version_id),
        Err(OutputError::AdapterCall(_))
    ));
    fixture.endpoint.failure = 0;
    fixture
        .deliver(&request.version_id, &approval.version_id)
        .unwrap();
    let calls = fixture.endpoint.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].invocation_id, calls[1].invocation_id);
    assert_eq!(fixture.endpoint.effects.lock().unwrap().len(), 1);
}

#[test]
fn malformed_receipts_are_terminal_and_never_commit_a_success_result() {
    for failure in 1..=3 {
        let mut fixture = Fixture::new();
        let request = fixture.prepare("report.write").unwrap();
        let approval = fixture.approve(&request.version_id, ApprovalDecision::Approved);
        fixture.endpoint.failure = failure;
        assert!(
            fixture
                .deliver(&request.version_id, &approval.version_id)
                .is_err()
        );
        assert!(matches!(
            fixture.deliver(&request.version_id, &approval.version_id),
            Err(OutputError::Invocation(_))
        ));
        assert_eq!(fixture.endpoint.calls.lock().unwrap().len(), 1);
        assert!(
            fixture
                .store
                .committed_after(0)
                .unwrap()
                .iter()
                .all(|entry| entry.artifact.schema_id.0 != RESULT)
        );
    }
}

#[test]
fn current_grants_are_checked_before_receipt_replay() {
    let mut fixture = Fixture::new();
    let request = fixture.prepare("report.write").unwrap();
    let approval = fixture.approve(&request.version_id, ApprovalDecision::Approved);
    fixture
        .deliver(&request.version_id, &approval.version_id)
        .unwrap();
    fixture.grant.enabled_capabilities = vec![CapabilityId("other.write".into())];
    assert!(matches!(
        fixture.deliver(&request.version_id, &approval.version_id),
        Err(OutputError::Authorization(_))
    ));
    assert_eq!(fixture.endpoint.calls.lock().unwrap().len(), 1);
}

#[test]
fn output_requires_executor_role_and_cannot_emit_control_artifacts() {
    let mut fixture = Fixture::new();
    fixture.grant.producer_class = ProducerClass::Adapter;
    assert!(matches!(
        fixture.prepare("report.write"),
        Err(OutputError::ExecutorRequired)
    ));
    fixture.grant.producer_class = ProducerClass::Executor;
    fixture.endpoint.manifest.capabilities[0].contract = CapabilityContract::Act {
        accepts: vec![SchemaId(INPUT.into())],
        emits: vec![SchemaId(RESULT.into())],
        idempotent: true,
    };
    assert!(matches!(
        fixture.prepare("report.write"),
        Err(OutputError::ContractMismatch)
    ));
    fixture.endpoint.manifest.capabilities[0].contract = CapabilityContract::Output {
        accepts: vec![SchemaId(INPUT.into())],
        emits: vec![SchemaId("arcs.output_request.v1".into())],
        idempotent: true,
    };
    let registry = fixture.registry();
    assert!(matches!(
        OutputService::new(
            &registry,
            &fixture.schemas,
            &fixture.store,
            &mut fixture.store.durable_id_generator("test").unwrap(),
            &SystemClock
        )
        .prepare(
            &CapabilityRef::new("output.test", "report.write"),
            &VersionId("report-v1".into()),
            &SchemaId("arcs.output_request.v1".into())
        ),
        Err(OutputError::ControlResultForbidden)
    ));
    assert_eq!(fixture.store.len().unwrap(), 1);
}
