use arcs_adapter_protocol::formal::{
    FormalCheckOutcome, FormalCheckRequest, FormalFinding, FormalFindingVerdict, FormalVerdict,
    Toolchain,
};
use serde_json::json;

use super::*;
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds,
    SchemaRegistry, Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, relation_kinds};

const TARGET_SCHEMA: &str = r#"{
    "$id": "arcs.action_candidate.formal_test.v1",
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
        "2026-11-28T09:00:00Z".into()
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

/// Ein formales Werkzeug mit einstellbarem Ergebnis.
struct FixedChecker {
    outcome: FormalCheckOutcome,
}

impl FixedChecker {
    fn proved() -> Self {
        Self {
            outcome: FormalCheckOutcome {
                verdict: FormalVerdict::Proved,
                used_axioms: vec!["propext".into()],
                findings: vec![FormalFinding {
                    check: "formal.no_sorry".into(),
                    verdict: FormalFindingVerdict::Pass,
                    detail: "the proof contains no placeholder".into(),
                }],
                proof_text: None,
                duration_ms: 12,
            },
        }
    }

    fn with_verdict(verdict: FormalVerdict) -> Self {
        let mut checker = Self::proved();
        checker.outcome.verdict = verdict;
        checker
    }

    fn with_sorry() -> Self {
        let mut checker = Self::proved();
        checker.outcome.findings = vec![FormalFinding {
            check: "formal.no_sorry".into(),
            verdict: FormalFindingVerdict::Fail,
            detail: "the proof contains an unproved placeholder".into(),
        }];
        checker
    }

    fn with_axioms(axioms: &[&str]) -> Self {
        let mut checker = Self::proved();
        checker.outcome.used_axioms = axioms.iter().map(|axiom| (*axiom).to_owned()).collect();
        checker
    }
}

impl FormalChecker for FixedChecker {
    fn check(&self, _request: &FormalCheckRequest) -> Result<FormalCheckOutcome, FormalError> {
        Ok(self.outcome.clone())
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(TARGET_SCHEMA).unwrap();
    schemas
}

fn target() -> Artifact {
    Artifact::new(
        "candidate-1",
        "candidate-1-v1",
        "action_candidate",
        "arcs.action_candidate.formal_test.v1",
        "2026-11-28T08:59:00Z",
        Actor {
            actor_type: ActorType::Model,
            id: "reasoner.test".into(),
        },
        Source {
            kind: SourceKind::External,
            reference: "reasoning:formal-test".into(),
        },
        Trust {
            level: TrustLevel::Low,
            source_class: SourceClass::Model,
        },
        "reasoning:formal-test",
        json!({"summary": "open the door"}),
    )
}

fn toolchain() -> Toolchain {
    Toolchain {
        name: "lean4".into(),
        version: "4.36.0".into(),
    }
}

fn obligation_draft(target_version: VersionId) -> ObligationDraft {
    ObligationDraft {
        name: "door-state".into(),
        statement: "state = open".into(),
        assumption_names: vec!["state".into()],
        definition_names: vec!["state".into()],
        target_version: Some(target_version),
        model_version: None,
        plan_revision: Some("plan-v1".into()),
        goal: Some("the door opens".into()),
        preconditions: vec!["the door is closed".into()],
        postconditions: vec!["the door is open".into()],
        invariants: vec!["the door stays closed before".into()],
        resource_limits: Some(ResourceLimits {
            max_model_calls: 2,
            max_tokens: 1_000,
            max_cost_micros: 10,
            max_wall_clock_ms: 5_000,
        }),
        allowed_axioms: vec!["propext".into()],
        source_versions: Vec::new(),
    }
}

/// Der Bezug, unter dem ein Nachweis ausgewertet wird.
///
/// Die Obligation bindet sich an Handlung, Planrevision und Ziel. Der Aufrufer
/// bestätigt denselben Kontext; ein fehlendes Feld bestätigt nichts.
fn binding(target: VersionId) -> EvidenceBinding {
    EvidenceBinding {
        target: Some(target),
        plan_revision: Some("plan-v1".into()),
        goal: Some("the door opens".into()),
    }
}

fn proof_draft(target_version: VersionId, obligation: &VersionId) -> ProofDraft {
    ProofDraft {
        name: "door-state".into(),
        statement: "state = open".into(),
        assumption_names: vec!["state".into()],
        definition_names: vec!["state".into()],
        obligation_version: Some(obligation.clone()),
        target_version: Some(target_version),
        model_version: None,
        plan_revision: Some("plan-v1".into()),
        toolchain: toolchain(),
        allowed_axioms: vec!["propext".into()],
        proof_text: "simp".into(),
        prover: Some("lean4".into()),
        source_versions: Vec::new(),
    }
}

fn seed(service: &mut FormalEvidenceService<'_>) -> VersionId {
    let assumption = service
        .record_assumption(&AssumptionDraft {
            name: "state".into(),
            statement: "state : Nat".into(),
            origin: AssumptionOrigin::User,
            source_versions: Vec::new(),
        })
        .unwrap();
    let definition = service
        .record_definition(&DefinitionDraft {
            name: "state".into(),
            body: "def state : Nat := 0".into(),
            source_versions: vec![assumption.version_id.clone()],
        })
        .unwrap();
    assert_eq!(definition.artifact_type, "formal_definition");
    assert_eq!(assumption.artifact_type, "formal_assumption");
    assumption.version_id
}

#[test]
fn a_proved_obligation_allows_the_action() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    assert_eq!(obligation.artifact_type, "proof_obligation");
    let proof = service
        .record_proof(&proof_draft(
            candidate.version_id.clone(),
            &obligation.version_id,
        ))
        .unwrap();
    let report = service
        .verify(&proof.version_id, &FixedChecker::proved())
        .unwrap();

    assert_eq!(report.artifact_type, "formal_verification_report");
    assert_eq!(report.payload["verdict"], json!("proved"));
    // Der Bericht nennt die Handlung, für die er gilt, und den geprüften
    // Beweisvorschlag, aus dem er hervorgegangen ist.
    assert_eq!(
        report.payload["target_version"],
        json!(candidate.version_id.0)
    );
    assert_eq!(report.payload["proof_version"], json!(proof.version_id.0));
    assert_eq!(
        report.payload["obligation_version"],
        json!(obligation.version_id.0)
    );

    let relations = store.outgoing_relations(&report.version_id).unwrap();
    assert!(
        relations
            .iter()
            .any(|relation| relation.to == proof.version_id
                && relation.kind == relation_kinds::verifies())
    );
    assert!(
        relations
            .iter()
            .any(|relation| relation.to == obligation.version_id
                && relation.kind == relation_kinds::based_on())
    );

    let decision = service
        .decide(
            &obligation.version_id,
            &binding(candidate.version_id.clone()),
        )
        .unwrap();
    assert!(decision.is_allowed(), "{decision:?}");
}

#[test]
fn a_proof_with_a_placeholder_is_not_accepted() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    let proof = service
        .record_proof(&proof_draft(
            candidate.version_id.clone(),
            &obligation.version_id,
        ))
        .unwrap();
    let report = service
        .verify(&proof.version_id, &FixedChecker::with_sorry())
        .unwrap();

    assert_eq!(report.payload["verdict"], json!("rejected"));
    let decision = service
        .decide(
            &obligation.version_id,
            &binding(candidate.version_id.clone()),
        )
        .unwrap();
    assert!(
        matches!(decision, EvidenceDecision::Blocked(_)),
        "{decision:?}"
    );
}

#[test]
fn a_proof_with_a_forbidden_axiom_is_not_accepted() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    let proof = service
        .record_proof(&proof_draft(
            candidate.version_id.clone(),
            &obligation.version_id,
        ))
        .unwrap();
    let report = service
        .verify(
            &proof.version_id,
            &FixedChecker::with_axioms(&["Classical.choice", "secretAxiom"]),
        )
        .unwrap();

    assert_eq!(report.payload["verdict"], json!("rejected"));
    let detail = report.payload["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["check"] == json!("formal.allowed_axioms"));
    assert!(detail, "{report:?}");
}

#[test]
fn an_unavailable_checker_creates_a_waiting_state() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    let proof = service
        .record_proof(&proof_draft(
            candidate.version_id.clone(),
            &obligation.version_id,
        ))
        .unwrap();
    service
        .verify(
            &proof.version_id,
            &FixedChecker::with_verdict(FormalVerdict::Unavailable),
        )
        .unwrap();

    let decision = service
        .decide(
            &obligation.version_id,
            &binding(candidate.version_id.clone()),
        )
        .unwrap();
    assert!(
        matches!(decision, EvidenceDecision::Waiting(_)),
        "{decision:?}"
    );
}

#[test]
fn a_failed_check_blocks_the_action() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    let proof = service
        .record_proof(&proof_draft(
            candidate.version_id.clone(),
            &obligation.version_id,
        ))
        .unwrap();
    service
        .verify(
            &proof.version_id,
            &FixedChecker::with_verdict(FormalVerdict::Failed),
        )
        .unwrap();

    let decision = service
        .decide(
            &obligation.version_id,
            &binding(candidate.version_id.clone()),
        )
        .unwrap();
    assert!(
        matches!(decision, EvidenceDecision::Blocked(_)),
        "{decision:?}"
    );
}

#[test]
fn a_missing_report_blocks_the_action() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();

    let decision = service
        .decide(
            &obligation.version_id,
            &binding(candidate.version_id.clone()),
        )
        .unwrap();
    assert!(
        matches!(decision, EvidenceDecision::Blocked(_)),
        "{decision:?}"
    );
}

#[test]
fn evidence_for_another_statement_does_not_help() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    let mut draft = proof_draft(candidate.version_id.clone(), &obligation.version_id);
    draft.statement = "state = closed".into();
    let proof = service.record_proof(&draft).unwrap();
    service
        .verify(&proof.version_id, &FixedChecker::proved())
        .unwrap();

    let decision = service
        .decide(
            &obligation.version_id,
            &binding(candidate.version_id.clone()),
        )
        .unwrap();
    assert!(
        matches!(decision, EvidenceDecision::Blocked(_)),
        "{decision:?}"
    );
}

#[test]
fn evidence_for_another_action_does_not_help() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    let other = Artifact::new(
        "candidate-2",
        "candidate-2-v1",
        "action_candidate",
        "arcs.action_candidate.formal_test.v1",
        "2026-11-28T08:59:00Z",
        Actor {
            actor_type: ActorType::Model,
            id: "reasoner.test".into(),
        },
        Source {
            kind: SourceKind::External,
            reference: "reasoning:formal-test-2".into(),
        },
        Trust {
            level: TrustLevel::Low,
            source_class: SourceClass::Model,
        },
        "reasoning:formal-test-2",
        json!({"summary": "close the door"}),
    );
    store.append(&candidate, &schemas).unwrap();
    store.append(&other, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(other.version_id.clone()))
        .unwrap();
    let proof = service
        .record_proof(&proof_draft(
            other.version_id.clone(),
            &obligation.version_id,
        ))
        .unwrap();
    service
        .verify(&proof.version_id, &FixedChecker::proved())
        .unwrap();

    let decision = service
        .decide(
            &obligation.version_id,
            &binding(candidate.version_id.clone()),
        )
        .unwrap();
    assert!(
        matches!(decision, EvidenceDecision::Blocked(_)),
        "{decision:?}"
    );
}

#[test]
fn an_unknown_assumption_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    let proof = service
        .record_proof(&proof_draft(
            candidate.version_id.clone(),
            &obligation.version_id,
        ))
        .unwrap();
    let error = service
        .verify(&proof.version_id, &FixedChecker::proved())
        .unwrap_err();
    assert!(matches!(error, FormalError::Invalid(_)), "{error:?}");
}

#[test]
fn a_proof_may_not_use_more_axioms_than_the_obligation_allows() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    let mut draft = proof_draft(candidate.version_id.clone(), &obligation.version_id);
    draft.allowed_axioms = vec!["propext".into(), "Classical.choice".into()];

    let error = service.record_proof(&draft).unwrap_err();
    assert!(
        matches!(&error, FormalError::Invalid(detail) if detail.contains("Classical.choice")),
        "{error:?}"
    );
}

#[test]
fn a_proof_may_not_assume_more_than_the_obligation_allows() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    let mut draft = proof_draft(candidate.version_id.clone(), &obligation.version_id);
    draft.assumption_names = vec!["state".into(), "secret".into()];

    let error = service.record_proof(&draft).unwrap_err();
    assert!(
        matches!(&error, FormalError::Invalid(detail) if detail.contains("secret")),
        "{error:?}"
    );
}

#[test]
fn evidence_for_another_plan_revision_does_not_help() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    let proof = service
        .record_proof(&proof_draft(
            candidate.version_id.clone(),
            &obligation.version_id,
        ))
        .unwrap();
    service
        .verify(&proof.version_id, &FixedChecker::proved())
        .unwrap();

    let mut stale = binding(candidate.version_id.clone());
    stale.plan_revision = Some("plan-v0".into());
    let decision = service.decide(&obligation.version_id, &stale).unwrap();
    assert!(
        matches!(decision, EvidenceDecision::Blocked(_)),
        "{decision:?}"
    );
}

#[test]
fn evidence_for_another_goal_does_not_help() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    let proof = service
        .record_proof(&proof_draft(
            candidate.version_id.clone(),
            &obligation.version_id,
        ))
        .unwrap();
    service
        .verify(&proof.version_id, &FixedChecker::proved())
        .unwrap();

    let mut other = binding(candidate.version_id.clone());
    other.goal = Some("the door closes".into());
    let decision = service.decide(&obligation.version_id, &other).unwrap();
    assert!(
        matches!(decision, EvidenceDecision::Blocked(_)),
        "{decision:?}"
    );
}

#[test]
fn an_obligation_bound_to_a_plan_can_authorize_an_action() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let mut draft = obligation_draft(candidate.version_id.clone());
    // Die Obligation bindet sich nicht an ein einzelnes Artefakt, sondern an
    // Planrevision und Ziel. Das ist der Fall eines laufenden Ziels, dessen
    // Handlung erst zur Laufzeit entsteht.
    draft.target_version = None;
    let obligation = service.record_obligation(&draft).unwrap();
    let mut proof = proof_draft(candidate.version_id.clone(), &obligation.version_id);
    proof.target_version = None;
    let proof = service.record_proof(&proof).unwrap();
    service
        .verify(&proof.version_id, &FixedChecker::proved())
        .unwrap();

    let plan_binding = EvidenceBinding::plan(Some("plan-v1".into()), Some("the door opens".into()));
    let decision = service
        .decide(&obligation.version_id, &plan_binding)
        .unwrap();
    assert!(decision.is_allowed(), "{decision:?}");

    // Ein Aufrufer, der den Zielbezug nicht bestätigt, erhält keinen Nachweis.
    let unbound = service
        .decide(&obligation.version_id, &EvidenceBinding::default())
        .unwrap();
    assert!(
        matches!(unbound, EvidenceDecision::Blocked(_)),
        "{unbound:?}"
    );
}

#[test]
fn an_external_outcome_becomes_a_verifiable_report() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    let request = service
        .prepare_check(&obligation.version_id, 5_000)
        .unwrap();
    assert_eq!(request.artifact_type, "formal_check_request");
    // Die Anfrage löst die benannten Definitionen und Annahmen auf.
    assert_eq!(request.payload["statement"], json!("state = open"));
    assert_eq!(request.payload["definitions"][0]["name"], json!("state"));
    assert_eq!(request.payload["assumptions"][0]["name"], json!("state"));

    let outcome = FormalCheckOutcome {
        verdict: FormalVerdict::Proved,
        used_axioms: vec!["propext".into()],
        findings: vec![],
        proof_text: Some("simp".into()),
        duration_ms: 12,
    };
    let report = service
        .record_outcome(&request.version_id, outcome, toolchain())
        .unwrap();
    assert_eq!(report.payload["verdict"], json!("proved"));
    assert_eq!(
        report.payload["obligation_version"],
        json!(obligation.version_id.0)
    );

    let decision = service
        .decide(
            &obligation.version_id,
            &binding(candidate.version_id.clone()),
        )
        .unwrap();
    assert!(decision.is_allowed(), "{decision:?}");
}

#[test]
fn an_external_outcome_without_a_proof_is_not_a_proof() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    let request = service
        .prepare_check(&obligation.version_id, 5_000)
        .unwrap();
    let outcome = FormalCheckOutcome {
        verdict: FormalVerdict::Proved,
        used_axioms: vec![],
        findings: vec![],
        proof_text: None,
        duration_ms: 3,
    };
    let report = service
        .record_outcome(&request.version_id, outcome, toolchain())
        .unwrap();

    // Ein "bewiesen" ohne vorgelegten Beweis ist kein Nachweis.
    assert_eq!(report.payload["verdict"], json!("proved"));
    assert!(report.payload["proof_version"].is_null());
    let decision = service
        .decide(
            &obligation.version_id,
            &binding(candidate.version_id.clone()),
        )
        .unwrap();
    assert!(
        matches!(decision, EvidenceDecision::Blocked(_)),
        "{decision:?}"
    );
}

#[test]
fn an_unavailable_external_outcome_creates_a_waiting_state() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let candidate = target();
    store.append(&candidate, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut service = FormalEvidenceService::new(&schemas, &store, &mut ids, &FixedClock);
    seed(&mut service);

    let obligation = service
        .record_obligation(&obligation_draft(candidate.version_id.clone()))
        .unwrap();
    let request = service
        .prepare_check(&obligation.version_id, 5_000)
        .unwrap();
    let outcome = FormalCheckOutcome {
        verdict: FormalVerdict::Unavailable,
        used_axioms: vec![],
        findings: vec![],
        proof_text: None,
        duration_ms: 1,
    };
    let report = service
        .record_outcome(&request.version_id, outcome, toolchain())
        .unwrap();
    assert_eq!(report.payload["verdict"], json!("unavailable"));

    let decision = service
        .decide(
            &obligation.version_id,
            &binding(candidate.version_id.clone()),
        )
        .unwrap();
    assert!(
        matches!(decision, EvidenceDecision::Waiting(_)),
        "{decision:?}"
    );
}
