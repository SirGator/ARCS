use serde_json::json;

use super::{
    VerificationError, VerificationFinding, VerificationResult, VerificationService,
    VerificationVerdict, Verifier,
};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds,
    SchemaRegistry, Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId,
};
use crate::store::{ArtifactNetwork, SqliteArtifactStore, relation_kinds};

const CANDIDATE_SCHEMA: &str = r#"{
    "$id": "arcs.route_candidate.verification_test.v1",
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
        "2026-08-08T14:00:00Z".into()
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

impl FixedVerifier {
    fn pass() -> Self {
        Self {
            verdict: VerificationVerdict::Pass,
        }
    }

    fn unknown() -> Self {
        Self {
            verdict: VerificationVerdict::Unknown,
        }
    }
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

fn candidate() -> Artifact {
    Artifact::new(
        "candidate-1",
        "candidate-1-v1",
        "route_candidate",
        "arcs.route_candidate.verification_test.v1",
        "2026-08-08T13:59:00Z",
        Actor {
            actor_type: ActorType::Model,
            id: "reasoner.test".into(),
        },
        Source {
            kind: SourceKind::External,
            reference: "reasoning:verification-test".into(),
        },
        Trust {
            level: TrustLevel::Low,
            source_class: SourceClass::Model,
        },
        "reasoning:verification-test",
        json!({"summary": "open the door"}),
    )
}

#[test]
fn pass_report_is_persisted() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = candidate();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);

    let report = VerificationService::new(&schemas, &store, &mut ids, &FixedClock)
        .verify(&candidate.version_id, &FixedVerifier::pass())
        .unwrap();

    assert_eq!(report.artifact_type, "verification_report");
    assert_eq!(report.created_by.actor_type, ActorType::System);
    assert_eq!(report.source.kind, SourceKind::Internal);
    assert_eq!(
        report.provenance.as_ref().unwrap().parents,
        vec![candidate.version_id.0.clone()]
    );
    assert_eq!(
        report.provenance.as_ref().unwrap().rules_applied,
        vec!["verification.executed"]
    );
    assert_eq!(
        report.payload,
        json!({
            "target_version": candidate.version_id.0,
            "verdict": "pass",
            "findings": [{
                "check": "permission",
                "verdict": "pass",
                "detail": "required capability is enabled"
            }]
        })
    );
    assert_eq!(store.get(&report.version_id).unwrap(), Some(report));
    assert_eq!(store.len().unwrap(), 2);
}

#[test]
fn verification_report_references_target() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = candidate();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);

    let report = VerificationService::new(&schemas, &store, &mut ids, &FixedClock)
        .verify(&candidate.version_id, &FixedVerifier::pass())
        .unwrap();

    let relations = store.outgoing_relations(&report.version_id).unwrap();
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].from, report.version_id);
    assert_eq!(relations[0].to, candidate.version_id);
    assert_eq!(relations[0].kind, relation_kinds::verifies());
    assert!(
        ArtifactNetwork::new(&store)
            .neighbors(&report.version_id)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn unknown_is_preserved() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = candidate();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);

    let report = VerificationService::new(&schemas, &store, &mut ids, &FixedClock)
        .verify(&candidate.version_id, &FixedVerifier::unknown())
        .unwrap();

    assert_eq!(report.payload["verdict"], json!("unknown"));
    assert_eq!(report.payload["findings"][0]["verdict"], json!("unknown"));
    assert_eq!(store.get(&report.version_id).unwrap(), Some(report));
}

#[test]
fn missing_target_cannot_be_verified() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut ids = TestIds(1);
    let missing = VersionId("missing-candidate-v1".into());

    let result = VerificationService::new(&schemas, &store, &mut ids, &FixedClock)
        .verify(&missing, &FixedVerifier::pass());

    assert!(matches!(
        result,
        Err(VerificationError::MissingTarget(version)) if version == missing
    ));
    assert_eq!(store.len().unwrap(), 0);
}
