//! Tests der kontrollierten Autonomie.
//!
//! Die Tests decken beide Seiten ab: die Einordnung einer Wirkung nach der
//! Betreiberregel und die Wirkung dieser Einordnung auf die bestehende
//! Freigabe- und Aktionskette. Negative Fälle prüfen, dass keine Freigabe
//! entsteht und dass die Entscheidung trotzdem protokolliert wird.

use serde_json::json;

use super::{
    ActionProfile, AutonomyBounds, AutonomyError, AutonomyLevel, AutonomyPolicy, AutonomyReason,
    AutonomyRequest, AutonomyService, BudgetPolicy, CapabilityRule, Environment, HumanTrigger,
    NonDelegableRule, RiskLevel,
};
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, AdapterRegistry,
    CapabilityContract, CapabilityDescriptor, CapabilityId, CapabilityRef, ProducerClass,
};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Clock, GeneratedArtifactIds,
    SchemaId, SchemaRegistry, Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId,
};
use crate::store::{SqliteArtifactStore, relation_kinds};
use crate::verification::{
    VerificationError, VerificationFinding, VerificationResult, VerificationService,
    VerificationVerdict, Verifier,
};

const CANDIDATE_SCHEMA_ID: &str = "arcs.autonomy_candidate.test.v1";
const RESULT_SCHEMA_ID: &str = "arcs.autonomy_result.test.v1";

const CANDIDATE_SCHEMA: &str = r#"{
    "$id": "arcs.autonomy_candidate.test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["command"],
    "properties": {"command": {"type": "string", "minLength": 1}},
    "additionalProperties": false
}"#;

const OTHER_CANDIDATE_SCHEMA_ID: &str = "arcs.autonomy_candidate.other.test.v1";

const OTHER_CANDIDATE_SCHEMA: &str = r#"{
    "$id": "arcs.autonomy_candidate.other.test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["command"],
    "properties": {"command": {"type": "string", "minLength": 1}},
    "additionalProperties": false
}"#;

const RESULT_SCHEMA: &str = r#"{
    "$id": "arcs.autonomy_result.test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["ok"],
    "properties": {"ok": {"type": "boolean"}},
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

struct PassVerifier;

impl Verifier for PassVerifier {
    fn verify(&self, _artifact: &Artifact) -> Result<VerificationResult, VerificationError> {
        Ok(VerificationResult {
            verdict: VerificationVerdict::Pass,
            findings: vec![VerificationFinding {
                check: "autonomy_safety".into(),
                verdict: VerificationVerdict::Pass,
                detail: "candidate may be materialized".into(),
            }],
        })
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(CANDIDATE_SCHEMA).unwrap();
    schemas.register_json(OTHER_CANDIDATE_SCHEMA).unwrap();
    schemas.register_json(RESULT_SCHEMA).unwrap();
    schemas
}

fn registry(schemas: &SchemaRegistry, store: &SqliteArtifactStore) -> AdapterRegistry {
    let mut registry = AdapterRegistry::new();
    registry
        .register(
            AdapterManifest {
                protocol_version: ADAPTER_PROTOCOL_VERSION,
                adapter_id: AdapterId("autonomy.test".into()),
                adapter_version: "1.0.0".into(),
                capabilities: vec![CapabilityDescriptor {
                    description: None,
                    id: CapabilityId("device.set".into()),
                    contract: CapabilityContract::Act {
                        accepts: vec![SchemaId(CANDIDATE_SCHEMA_ID.into())],
                        emits: vec![SchemaId(RESULT_SCHEMA_ID.into())],
                        idempotent: true,
                    },
                    required_permissions: vec!["device.write".into()],
                }],
            },
            AdapterGrant {
                boundary: None,
                adapter_id: AdapterId("autonomy.test".into()),
                producer_class: ProducerClass::Executor,
                enabled_capabilities: vec![CapabilityId("device.set".into())],
                granted_permissions: vec!["device.write".into()],
                assigned_trust: TrustLevel::High,
                ingress_source_kind: None,
                max_payload_bytes: 4096,
                max_external_reference_bytes: 256,
                data_limits: None,
                input_data_limits: None,
                reasoning_limits_v2: None,
                reasoning_limits: None,
                data_classification: None,
            },
            schemas,
            store,
        )
        .unwrap();
    registry
}

fn capability() -> CapabilityRef {
    CapabilityRef::new("autonomy.test", "device.set")
}

fn candidate(required: &CapabilityRef) -> Artifact {
    let mut candidate = Artifact::new(
        "candidate-1",
        "candidate-1-v1",
        "autonomy_candidate",
        CANDIDATE_SCHEMA_ID,
        "2026-11-28T08:59:00Z",
        Actor {
            actor_type: ActorType::Model,
            id: "llm.test".into(),
        },
        Source {
            kind: SourceKind::External,
            reference: "reasoning:autonomy-test".into(),
        },
        Trust {
            level: TrustLevel::Low,
            source_class: SourceClass::Model,
        },
        "reasoning:autonomy-test",
        json!({"command": "open door"}),
    );
    candidate.tags = vec![format!(
        "requires:{}:{}",
        required.adapter_id.0, required.capability_id.0
    )];
    candidate
}

fn policy() -> AutonomyPolicy {
    AutonomyPolicy {
        version: "2026-11-28.v1".into(),
        capabilities: vec![CapabilityRule {
            adapter_id: "autonomy.test".into(),
            capability_id: "device.set".into(),
            schema_id: Some(CANDIDATE_SCHEMA_ID.into()),
            baseline_risk: RiskLevel::Low,
            environments: vec![Environment::Development, Environment::Test],
            max_cost: Some(100),
            expected_cost: Some(10),
            currency: Some("EUR".into()),
        }],
        routine: AutonomyBounds {
            max_risk: RiskLevel::Low,
            max_cost: Some(100),
            environments: vec![Environment::Development, Environment::Test],
        },
        bounded: AutonomyBounds {
            max_risk: RiskLevel::Medium,
            max_cost: Some(1000),
            environments: vec![Environment::Development, Environment::Test],
        },
        human_triggers: Vec::new(),
        non_delegable: Vec::new(),
        budget: Some(BudgetPolicy {
            budget_id: "ops-2026".into(),
            currency: "EUR".into(),
            limit: 1000,
        }),
    }
}

fn profile(risk: RiskLevel, environment: Environment, cost: Option<u64>) -> ActionProfile {
    ActionProfile {
        capability: capability(),
        schema_id: Some(SchemaId(CANDIDATE_SCHEMA_ID.into())),
        risk,
        environment,
        cost,
        currency: Some("EUR".into()),
        summary: "open the door".into(),
    }
}

fn codes(reasons: &[AutonomyReason]) -> Vec<&'static str> {
    reasons
        .iter()
        .map(|reason| match reason {
            AutonomyReason::WithinRoutineBounds => "within_routine_bounds",
            AutonomyReason::WithinBoundedBounds => "within_bounded_bounds",
            AutonomyReason::UnknownCapability => "unknown_capability",
            AutonomyReason::EnvironmentNotAllowed => "environment_not_allowed",
            AutonomyReason::RiskAboveBound => "risk_above_bound",
            AutonomyReason::CostAboveBound => "cost_above_bound",
            AutonomyReason::UnknownCost => "unknown_cost",
            AutonomyReason::CurrencyMismatch => "currency_mismatch",
            AutonomyReason::HumanTrigger { .. } => "human_trigger",
            AutonomyReason::NonDelegable { .. } => "non_delegable",
            AutonomyReason::BudgetExhausted => "budget_exhausted",
            AutonomyReason::BudgetExceeded => "budget_exceeded",
            AutonomyReason::NoBudgetConfigured => "no_budget_configured",
        })
        .collect()
}

#[test]
fn routine_work_inside_the_operator_bounds_is_allowed() {
    let assessment = AutonomyService::assess(
        &policy(),
        &profile(RiskLevel::Low, Environment::Test, Some(20)),
    );

    assert_eq!(assessment.level, AutonomyLevel::Routine);
    assert_eq!(codes(&assessment.reasons), vec!["within_routine_bounds"]);
}

#[test]
fn an_unknown_capability_is_never_autonomous() {
    let mut profile = profile(RiskLevel::Low, Environment::Test, Some(20));
    profile.capability = CapabilityRef::new("autonomy.test", "device.unknown");

    let assessment = AutonomyService::assess(&policy(), &profile);

    assert_eq!(assessment.level, AutonomyLevel::HumanDecision);
    assert_eq!(codes(&assessment.reasons), vec!["unknown_capability"]);
}

#[test]
fn the_operator_risk_dominates_the_claim_of_the_proposal() {
    let mut policy = policy();
    policy.capabilities[0].baseline_risk = RiskLevel::High;
    policy.bounded.max_risk = RiskLevel::High;

    let assessment = AutonomyService::assess(
        &policy,
        &profile(RiskLevel::Low, Environment::Test, Some(20)),
    );

    // Das Risiko des Vorschlags ist gering, das des Betreibers nicht. Die
    // Wirkung darf deshalb höchstens unter der bedingten Stufe laufen.
    assert_eq!(assessment.level, AutonomyLevel::Bounded);

    policy.bounded.max_risk = RiskLevel::Medium;
    let assessment = AutonomyService::assess(
        &policy,
        &profile(RiskLevel::Low, Environment::Test, Some(20)),
    );
    assert_eq!(assessment.level, AutonomyLevel::HumanDecision);
    assert_eq!(codes(&assessment.reasons), vec!["risk_above_bound"]);
}

#[test]
fn a_missing_price_without_operator_expectation_is_never_autonomous() {
    let mut policy = policy();
    policy.capabilities[0].expected_cost = None;

    let assessment =
        AutonomyService::assess(&policy, &profile(RiskLevel::Low, Environment::Test, None));

    assert_eq!(assessment.level, AutonomyLevel::HumanDecision);
    assert_eq!(codes(&assessment.reasons), vec!["unknown_cost"]);
}

#[test]
fn a_price_above_the_bound_is_never_autonomous() {
    let assessment = AutonomyService::assess(
        &policy(),
        &profile(RiskLevel::Low, Environment::Test, Some(5000)),
    );

    assert_eq!(assessment.level, AutonomyLevel::HumanDecision);
    assert_eq!(codes(&assessment.reasons), vec!["cost_above_bound"]);
}

#[test]
fn a_currency_mismatch_is_never_autonomous() {
    let mut profile = profile(RiskLevel::Low, Environment::Test, Some(20));
    profile.currency = Some("USD".into());

    let assessment = AutonomyService::assess(&policy(), &profile);

    assert_eq!(assessment.level, AutonomyLevel::HumanDecision);
    assert_eq!(codes(&assessment.reasons), vec!["currency_mismatch"]);
}

#[test]
fn production_needs_an_explicit_operator_permission() {
    let assessment = AutonomyService::assess(
        &policy(),
        &profile(RiskLevel::Low, Environment::Production, Some(20)),
    );

    assert_eq!(assessment.level, AutonomyLevel::HumanDecision);
    assert_eq!(codes(&assessment.reasons), vec!["environment_not_allowed"]);
}

#[test]
fn a_human_trigger_overrides_the_bounds() {
    let mut policy = policy();
    policy.human_triggers.push(HumanTrigger {
        adapter_id: Some("autonomy.test".into()),
        capability_id: Some("device.set".into()),
        schema_id: None,
        min_risk: None,
        min_cost: None,
        environment: Some(Environment::Test),
        reason: "production devices need a human".into(),
    });

    let assessment = AutonomyService::assess(
        &policy,
        &profile(RiskLevel::Low, Environment::Test, Some(20)),
    );

    assert_eq!(assessment.level, AutonomyLevel::HumanDecision);
    assert_eq!(codes(&assessment.reasons), vec!["human_trigger"]);
}

#[test]
fn a_non_delegable_rule_overrides_the_bounds() {
    let mut policy = policy();
    policy.non_delegable.push(NonDelegableRule {
        adapter_id: Some("autonomy.test".into()),
        capability_id: Some("device.set".into()),
        schema_id: None,
        environment: None,
        reason: "statutory human responsibility".into(),
    });

    let assessment = AutonomyService::assess(
        &policy,
        &profile(RiskLevel::Low, Environment::Test, Some(20)),
    );

    assert_eq!(assessment.level, AutonomyLevel::HumanDecision);
    assert_eq!(codes(&assessment.reasons), vec!["non_delegable"]);
}

#[test]
fn the_proposal_cannot_understate_the_price_below_the_operator_expectation() {
    let mut policy = policy();
    policy.routine.max_cost = Some(5);
    policy.bounded.max_cost = Some(5);

    let assessment = AutonomyService::assess(
        &policy,
        &profile(RiskLevel::Low, Environment::Test, Some(0)),
    );

    // Der Betreiber erwartet mindestens zehn Einheiten. Die Nullangabe des
    // Vorschlags hebt diese Erwartung nicht auf.
    assert_eq!(assessment.level, AutonomyLevel::HumanDecision);
    assert_eq!(codes(&assessment.reasons), vec!["cost_above_bound"]);
}

#[test]
fn bounded_autonomy_needs_a_budget() {
    let mut policy = policy();
    policy.budget = None;
    policy.routine.max_cost = Some(0);
    policy.routine.max_risk = RiskLevel::Low;

    let assessment = AutonomyService::assess(
        &policy,
        &profile(RiskLevel::Medium, Environment::Test, Some(200)),
    );

    assert_eq!(assessment.level, AutonomyLevel::Bounded);
}

#[test]
fn an_authorized_routine_action_yields_a_system_approval() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut ids = TestIds(1);
    let candidate = candidate(&capability());
    store.append(&candidate, &schemas).unwrap();
    let report = VerificationService::new(&schemas, &store, &mut ids, &FixedClock)
        .verify(&candidate.version_id, &PassVerifier)
        .unwrap();
    let mut service = AutonomyService::new(&schemas, &store, &mut ids, &FixedClock);
    let policy_artifact = service.publish_policy(&policy()).unwrap();
    let profile = profile(RiskLevel::Low, Environment::Test, Some(20));

    let authorization = service
        .authorize(&AutonomyRequest {
            policy_version: &policy_artifact.version_id,
            candidate_version: &candidate.version_id,
            verification_report_version: &report.version_id,
            profile: &profile,
        })
        .unwrap();

    assert_eq!(authorization.level, AutonomyLevel::Routine);
    assert_eq!(authorization.reserved_cost, 20);

    let approval = store.get(&authorization.approval_version).unwrap().unwrap();
    assert_eq!(approval.created_by.actor_type, ActorType::System);
    assert_eq!(approval.schema_id, SchemaId("arcs.approval.v1".into()));
    let relations = store.outgoing_relations(&approval.version_id).unwrap();
    assert!(relations.iter().any(|relation| {
        relation.to == candidate.version_id && relation.kind == relation_kinds::approves()
    }));
    assert!(relations.iter().any(|relation| {
        relation.to == report.version_id && relation.kind == relation_kinds::based_on()
    }));
    assert!(relations.iter().any(|relation| {
        relation.to == authorization.decision_version && relation.kind == relation_kinds::based_on()
    }));

    let budget = service.budget("ops-2026").unwrap().unwrap();
    assert_eq!(budget.spent, 20);
    assert_eq!(budget.remaining, 980);
}

#[test]
fn the_existing_action_chain_accepts_an_autonomous_approval() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let registry = registry(&schemas, &store);
    let mut ids = TestIds(1);
    let candidate = candidate(&capability());
    store.append(&candidate, &schemas).unwrap();
    let report = VerificationService::new(&schemas, &store, &mut ids, &FixedClock)
        .verify(&candidate.version_id, &PassVerifier)
        .unwrap();
    let mut service = AutonomyService::new(&schemas, &store, &mut ids, &FixedClock);
    let policy_artifact = service.publish_policy(&policy()).unwrap();
    let profile = profile(RiskLevel::Low, Environment::Test, Some(20));
    let authorization = service
        .authorize(&AutonomyRequest {
            policy_version: &policy_artifact.version_id,
            candidate_version: &candidate.version_id,
            verification_report_version: &report.version_id,
            profile: &profile,
        })
        .unwrap();

    let action =
        crate::action::ActionService::new(&registry, &schemas, &store, &mut ids, &FixedClock)
            .materialize(&authorization.approval_version, &capability())
            .unwrap();

    assert_eq!(action.artifact_type, "action");
    assert_eq!(
        action.payload["approval_version"],
        json!(authorization.approval_version.0)
    );
}

#[test]
fn an_escalated_action_leaves_no_approval_but_records_the_decision() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut ids = TestIds(1);
    let candidate = candidate(&capability());
    store.append(&candidate, &schemas).unwrap();
    let report = VerificationService::new(&schemas, &store, &mut ids, &FixedClock)
        .verify(&candidate.version_id, &PassVerifier)
        .unwrap();
    let mut service = AutonomyService::new(&schemas, &store, &mut ids, &FixedClock);
    let policy_artifact = service.publish_policy(&policy()).unwrap();
    let profile = profile(RiskLevel::Low, Environment::Production, Some(20));

    let error = service
        .authorize(&AutonomyRequest {
            policy_version: &policy_artifact.version_id,
            candidate_version: &candidate.version_id,
            verification_report_version: &report.version_id,
            profile: &profile,
        })
        .unwrap_err();

    let AutonomyError::HumanDecisionRequired {
        decision_version,
        reasons,
    } = &error
    else {
        panic!("expected an escalation, got {error:?}");
    };
    assert_eq!(
        reasons.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["environment_not_allowed"]
    );

    let decision = store.get(decision_version).unwrap().unwrap();
    assert_eq!(
        decision.schema_id,
        SchemaId("arcs.autonomy_decision.v1".into())
    );
    assert_eq!(decision.payload["level"], json!("human_decision"));

    // Die Entscheidung ist über den Dienst lesbar und nennt die Gründe.
    let recorded = service.decision(decision_version).unwrap();
    assert_eq!(recorded.level, AutonomyLevel::HumanDecision);
    assert_eq!(recorded.reasons, vec!["environment_not_allowed".to_owned()]);
    assert_eq!(recorded.risk, RiskLevel::Low);
    assert_eq!(recorded.environment, Environment::Production);
    assert_eq!(recorded.cost, Some(20));
    assert_eq!(recorded.adapter_id, "autonomy.test");
    assert_eq!(recorded.capability_id, "device.set");

    let history = service
        .decisions(&candidate.version_id, &policy_artifact.version_id)
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].decision_id, recorded.decision_id);

    // Keine Freigabe und keine Kostenbuchung.
    let approvals = store
        .history(
            &crate::core::SubjectId(format!("approval:{}", candidate.version_id.0)),
            &SchemaId("arcs.approval.v1".into()),
        )
        .unwrap();
    assert!(approvals.is_empty());
    assert!(service.budget("ops-2026").unwrap().is_none());
}

#[test]
fn the_budget_stops_the_second_expensive_action() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut ids = TestIds(1);
    let mut policy = policy();
    policy.budget = Some(BudgetPolicy {
        budget_id: "ops-2026".into(),
        currency: "EUR".into(),
        limit: 30,
    });
    let mut candidates = Vec::new();
    for index in 0..2 {
        let mut candidate = candidate(&capability());
        candidate.artifact_id = ArtifactId(format!("candidate-{index}"));
        candidate.version_id = VersionId(format!("candidate-{index}-v1"));
        store.append(&candidate, &schemas).unwrap();
        let report = VerificationService::new(&schemas, &store, &mut ids, &FixedClock)
            .verify(&candidate.version_id, &PassVerifier)
            .unwrap();
        candidates.push((candidate, report));
    }

    let mut service = AutonomyService::new(&schemas, &store, &mut ids, &FixedClock);
    let policy_artifact = service.publish_policy(&policy).unwrap();

    let first = candidates[0].clone();
    let profile = profile(RiskLevel::Low, Environment::Test, Some(20));
    service
        .authorize(&AutonomyRequest {
            policy_version: &policy_artifact.version_id,
            candidate_version: &first.0.version_id,
            verification_report_version: &first.1.version_id,
            profile: &profile,
        })
        .unwrap();

    let second = candidates[1].clone();
    let error = service
        .authorize(&AutonomyRequest {
            policy_version: &policy_artifact.version_id,
            candidate_version: &second.0.version_id,
            verification_report_version: &second.1.version_id,
            profile: &profile,
        })
        .unwrap_err();

    let AutonomyError::HumanDecisionRequired { reasons, .. } = &error else {
        panic!("expected an escalation, got {error:?}");
    };
    assert_eq!(
        reasons.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["within_routine_bounds", "budget_exceeded"]
    );

    // Der zweite Versuch hat keine Kosten gebucht.
    let budget = service.budget("ops-2026").unwrap().unwrap();
    assert_eq!(budget.spent, 20);
}

#[test]
fn a_failed_approval_releases_the_reserved_budget() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut ids = TestIds(1);
    let candidate = candidate(&capability());
    store.append(&candidate, &schemas).unwrap();
    let mut service = AutonomyService::new(&schemas, &store, &mut ids, &FixedClock);
    let policy_artifact = service.publish_policy(&policy()).unwrap();
    let profile = profile(RiskLevel::Low, Environment::Test, Some(20));

    // Ohne Prüfbericht kann keine Freigabe entstehen. Die Buchung muss
    // zurückgenommen werden.
    let error = service
        .authorize(&AutonomyRequest {
            policy_version: &policy_artifact.version_id,
            candidate_version: &candidate.version_id,
            verification_report_version: &VersionId("missing-report-v1".into()),
            profile: &profile,
        })
        .unwrap_err();

    assert!(matches!(error, AutonomyError::Approval(_)));
    let budget = service.budget("ops-2026").unwrap().unwrap();
    assert_eq!(budget.spent, 0);
}

#[test]
fn bounded_autonomy_without_a_budget_is_escalated() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut ids = TestIds(1);
    let candidate = candidate(&capability());
    store.append(&candidate, &schemas).unwrap();
    let report = VerificationService::new(&schemas, &store, &mut ids, &FixedClock)
        .verify(&candidate.version_id, &PassVerifier)
        .unwrap();
    let mut policy = policy();
    policy.budget = None;
    policy.routine.max_cost = Some(0);
    let mut service = AutonomyService::new(&schemas, &store, &mut ids, &FixedClock);
    let policy_artifact = service.publish_policy(&policy).unwrap();
    let profile = profile(RiskLevel::Medium, Environment::Test, Some(200));

    let error = service
        .authorize(&AutonomyRequest {
            policy_version: &policy_artifact.version_id,
            candidate_version: &candidate.version_id,
            verification_report_version: &report.version_id,
            profile: &profile,
        })
        .unwrap_err();

    let AutonomyError::HumanDecisionRequired { reasons, .. } = &error else {
        panic!("expected an escalation, got {error:?}");
    };
    assert_eq!(
        reasons.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["within_bounded_bounds", "no_budget_configured"]
    );
    assert!(service.budget("ops-2026").unwrap().is_none());
}

#[test]
fn a_candidate_for_another_capability_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut ids = TestIds(1);
    let mut candidate = candidate(&CapabilityRef::new("autonomy.test", "device.other"));
    candidate.artifact_id = ArtifactId("candidate-other".into());
    candidate.version_id = VersionId("candidate-other-v1".into());
    store.append(&candidate, &schemas).unwrap();
    let report = VerificationService::new(&schemas, &store, &mut ids, &FixedClock)
        .verify(&candidate.version_id, &PassVerifier)
        .unwrap();
    let mut service = AutonomyService::new(&schemas, &store, &mut ids, &FixedClock);
    let policy_artifact = service.publish_policy(&policy()).unwrap();
    let profile = profile(RiskLevel::Low, Environment::Test, Some(20));

    let error = service
        .authorize(&AutonomyRequest {
            policy_version: &policy_artifact.version_id,
            candidate_version: &candidate.version_id,
            verification_report_version: &report.version_id,
            profile: &profile,
        })
        .unwrap_err();

    assert!(matches!(error, AutonomyError::Invalid(_)));
    assert!(service.budget("ops-2026").unwrap().is_none());
}

#[test]
fn a_candidate_with_another_contract_is_rejected() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut ids = TestIds(1);
    let mut candidate = candidate(&capability());
    candidate.artifact_id = ArtifactId("candidate-contract".into());
    candidate.version_id = VersionId("candidate-contract-v1".into());
    candidate.schema_id = SchemaId(OTHER_CANDIDATE_SCHEMA_ID.into());
    store.append(&candidate, &schemas).unwrap();
    let report = VerificationService::new(&schemas, &store, &mut ids, &FixedClock)
        .verify(&candidate.version_id, &PassVerifier)
        .unwrap();
    let mut service = AutonomyService::new(&schemas, &store, &mut ids, &FixedClock);
    let policy_artifact = service.publish_policy(&policy()).unwrap();
    // Der Vorschlag behauptet einen anderen Vertrag als der Kandidat.
    let profile = profile(RiskLevel::Low, Environment::Test, Some(20));

    let error = service
        .authorize(&AutonomyRequest {
            policy_version: &policy_artifact.version_id,
            candidate_version: &candidate.version_id,
            verification_report_version: &report.version_id,
            profile: &profile,
        })
        .unwrap_err();

    assert!(matches!(error, AutonomyError::Invalid(_)));
    assert!(service.budget("ops-2026").unwrap().is_none());
}

#[test]
fn a_policy_without_any_environment_grants_nothing() {
    let mut policy = policy();
    policy.routine.environments = Vec::new();
    policy.bounded.environments = Vec::new();

    assert!(policy.validate().is_err());
}
