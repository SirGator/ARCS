use serde_json::json;

use super::{ApprovalDecision, ApprovalError, ApprovalService};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds,
    SchemaRegistry, Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId,
};
use crate::store::{ArtifactNetwork, SqliteArtifactStore, relation_kinds};
use crate::verification::{
    VerificationError, VerificationFinding, VerificationResult, VerificationService,
    VerificationVerdict, Verifier,
};

const CANDIDATE_SCHEMA: &str = r#"{
    "$id": "arcs.route_candidate.approval_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["summary"],
    "properties": {
        "summary": {"type": "string", "minLength": 1}
    },
    "additionalProperties": false
}"#;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-08-08T15:00:00Z".into()
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

struct FixedVerifier {
    verdict: VerificationVerdict,
}

impl Verifier for FixedVerifier {
    fn verify(&self, _artifact: &Artifact) -> Result<VerificationResult, VerificationError> {
        let detail = match self.verdict {
            VerificationVerdict::Pass => "required capability is enabled",
            VerificationVerdict::Fail => "required capability is disabled",
            VerificationVerdict::Unknown => "available evidence is inconclusive",
        };
        Ok(VerificationResult {
            verdict: self.verdict,
            findings: vec![VerificationFinding {
                check: "permission".into(),
                verdict: self.verdict,
                detail: detail.into(),
            }],
        })
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(CANDIDATE_SCHEMA).unwrap();
    schemas
}

fn candidate(number: u64) -> Artifact {
    Artifact::new(
        format!("candidate-{number}"),
        format!("candidate-{number}-v1"),
        "route_candidate",
        "arcs.route_candidate.approval_test.v1",
        "2026-08-08T14:59:00Z",
        Actor {
            actor_type: ActorType::Model,
            id: "llm.test".into(),
        },
        Source {
            kind: SourceKind::External,
            reference: format!("reasoning:approval-test:{number}"),
        },
        Trust {
            level: TrustLevel::Low,
            source_class: SourceClass::Model,
        },
        format!("reasoning:approval-test:{number}"),
        json!({"summary": "open the door"}),
    )
}

fn human_approver() -> Actor {
    Actor {
        actor_type: ActorType::Human,
        id: "operator.test".into(),
    }
}

fn verification_report(
    schemas: &SchemaRegistry,
    store: &SqliteArtifactStore,
    ids: &mut dyn ArtifactIdGenerator,
    candidate: &Artifact,
    verdict: VerificationVerdict,
) -> Artifact {
    VerificationService::new(schemas, store, ids, &FixedClock)
        .verify(&candidate.version_id, &FixedVerifier { verdict })
        .unwrap()
}

#[test]
fn passed_candidate_can_be_approved() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = candidate(1);
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let report = verification_report(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        VerificationVerdict::Pass,
    );
    let approver = human_approver();

    let approval = ApprovalService::new(&schemas, &store, &mut ids, &FixedClock)
        .decide(
            &candidate.version_id,
            &report.version_id,
            ApprovalDecision::Approved,
            approver.clone(),
            "all required checks passed".into(),
        )
        .unwrap();

    assert_eq!(approval.artifact_type, "approval");
    assert_eq!(approval.schema_id.0, "arcs.approval.v1");
    assert_eq!(approval.created_by, approver);
    assert_eq!(approval.source.kind, SourceKind::Internal);
    assert_eq!(
        approval.payload,
        json!({
            "target_version": candidate.version_id.0,
            "verification_report_version": report.version_id.0,
            "decision": "approved",
            "reason": "all required checks passed"
        })
    );
    let provenance = approval.provenance.as_ref().unwrap();
    assert_eq!(
        provenance.parents,
        vec![candidate.version_id.0.clone(), report.version_id.0.clone()]
    );
    assert_eq!(provenance.rules_applied, vec!["approval.decision"]);
    assert_eq!(
        store.get(&approval.version_id).unwrap(),
        Some(approval.clone())
    );

    let relations = store.outgoing_relations(&approval.version_id).unwrap();
    assert_eq!(relations.len(), 2);
    assert!(relations.iter().any(|relation| {
        relation.to == candidate.version_id && relation.kind == relation_kinds::approves()
    }));
    assert!(relations.iter().any(|relation| {
        relation.to == report.version_id && relation.kind == relation_kinds::based_on()
    }));
    assert!(
        ArtifactNetwork::new(&store)
            .neighbors(&approval.version_id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.len().unwrap(), 3);
}

#[test]
fn failed_candidate_cannot_be_approved() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = candidate(1);
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let report = verification_report(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        VerificationVerdict::Fail,
    );

    let result = ApprovalService::new(&schemas, &store, &mut ids, &FixedClock).decide(
        &candidate.version_id,
        &report.version_id,
        ApprovalDecision::Approved,
        human_approver(),
        "approve despite failed check".into(),
    );

    assert!(matches!(
        result,
        Err(ApprovalError::CannotApproveFailedVerification)
    ));
    assert_eq!(store.len().unwrap(), 2);
}

#[test]
fn unknown_candidate_cannot_be_approved() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = candidate(1);
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let report = verification_report(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        VerificationVerdict::Unknown,
    );

    let result = ApprovalService::new(&schemas, &store, &mut ids, &FixedClock).decide(
        &candidate.version_id,
        &report.version_id,
        ApprovalDecision::Approved,
        human_approver(),
        "approve despite inconclusive evidence".into(),
    );

    assert!(matches!(
        result,
        Err(ApprovalError::CannotApproveUnknownVerification)
    ));
    assert_eq!(store.len().unwrap(), 2);
}

#[test]
fn verification_for_another_candidate_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let first_candidate = candidate(1);
    let other_candidate = candidate(2);
    store.append(&first_candidate, &schemas).unwrap();
    store.append(&other_candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let other_report = verification_report(
        &schemas,
        &store,
        &mut ids,
        &other_candidate,
        VerificationVerdict::Pass,
    );

    let result = ApprovalService::new(&schemas, &store, &mut ids, &FixedClock).decide(
        &first_candidate.version_id,
        &other_report.version_id,
        ApprovalDecision::Approved,
        human_approver(),
        "all required checks passed".into(),
    );

    assert!(matches!(
        result,
        Err(ApprovalError::VerificationTargetMismatch)
    ));
    assert_eq!(store.len().unwrap(), 3);
}

#[test]
fn model_cannot_approve_its_own_candidate() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = candidate(1);
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let report = verification_report(
        &schemas,
        &store,
        &mut ids,
        &candidate,
        VerificationVerdict::Pass,
    );

    let result = ApprovalService::new(&schemas, &store, &mut ids, &FixedClock).decide(
        &candidate.version_id,
        &report.version_id,
        ApprovalDecision::Approved,
        candidate.created_by.clone(),
        "model self-approval".into(),
    );

    assert!(matches!(result, Err(ApprovalError::UnauthorizedApprover)));
    assert_eq!(store.len().unwrap(), 2);
}
