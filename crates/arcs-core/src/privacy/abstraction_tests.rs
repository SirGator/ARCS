use super::abstraction::*;
use super::abstraction_service::*;
use crate::adapters::CapabilityRef;
use crate::core::{
    Actor, ActorType, Artifact, Classification, Clock, SchemaRegistry, SecurityProfile,
    SequenceIdGenerator, Source, SourceClass, SourceKind, Trust, TrustLevel, VersionId,
};
use crate::store::SqliteArtifactStore;
use serde_json::{Value, json};

/// Verwaltete Uhr für die Artefakte der Tests.
struct Time {
    seconds: std::sync::atomic::AtomicU64,
}

impl Clock for Time {
    fn now_rfc3339(&self) -> String {
        crate::core::artifact_factory::format_rfc3339(
            self.seconds.load(std::sync::atomic::Ordering::SeqCst),
        )
    }
}

/// Vertrag der Testquelle.
const SOURCE_SCHEMA: &str = "arcs.customer_request.test.v1";

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas
        .register_json(
            r#"{"$id":"arcs.customer_request.test.v1","type":"object","required":["customer_id","amount","request"],"properties":{"customer_id":{"type":"string"},"amount":{"type":"number"},"request":{"type":"string"},"internal_note":{"type":"string"}},"additionalProperties":false}"#,
        )
        .unwrap();
    schemas
}

fn store() -> SqliteArtifactStore {
    SqliteArtifactStore::in_memory().unwrap()
}

fn reasoner() -> CapabilityRef {
    CapabilityRef::new("peer", "propose")
}

fn policy() -> AbstractionPolicy {
    AbstractionPolicy {
        purpose: "resolve customer request".into(),
        target_classification: Classification::Internal,
        reasoner: reasoner(),
        abstract_fields: vec![
            AbstractedField {
                field: "customer_id".into(),
                kind: VariableKind::Identifier,
            },
            AbstractedField {
                field: "amount".into(),
                kind: VariableKind::Number,
            },
        ],
        visible_fields: vec!["request".into()],
        max_variables: 8,
    }
}

fn payload() -> Value {
    json!({
        "customer_id": "C-4711",
        "amount": 12500,
        "request": "please refund",
        "internal_note": "not for the model",
    })
}

#[test]
fn an_abstract_problem_carries_no_concrete_value() {
    let abstraction = abstract_payload(&payload(), &policy()).unwrap();

    // The values leave the core only through the local binding table.
    let problem = serde_json::to_string(&abstraction.problem).unwrap();
    assert!(!problem.contains("C-4711"));
    assert!(!problem.contains("12500"));
    assert!(!problem.contains("not for the model"));
    assert_eq!(abstraction.bindings.entries.len(), 2);
    assert_eq!(abstraction.bindings.entries[0].value, json!("C-4711"));
    assert_eq!(abstraction.bindings.entries[1].value, json!(12500));

    // The structure survives, only the values are replaced.
    assert_eq!(abstraction.problem["request"], json!("please refund"));
    assert_eq!(
        abstraction.problem["customer_id"],
        json!({"$var": "v1"}),
        "the field keeps its place in the structure"
    );
    assert_eq!(abstraction.variables[0].kind, VariableKind::Identifier);
}

#[test]
fn a_field_outside_the_release_never_reaches_the_problem() {
    let abstraction = abstract_payload(&payload(), &policy()).unwrap();
    let problem = serde_json::to_string(&abstraction.problem).unwrap();
    assert!(!problem.contains("internal_note"));
}

#[test]
fn a_visible_field_that_repeats_a_sensitive_value_is_refused() {
    let mut policy = policy();
    policy.visible_fields.push("summary".into());
    let mut payload = payload();
    payload["summary"] = json!("customer C-4711 wants a refund");

    let error = abstract_payload(&payload, &policy).unwrap_err();
    assert!(
        matches!(error, AbstractionError::DisclosureDetected { .. }),
        "the transformation must not release a value it still carries: {error:?}"
    );
}

#[test]
fn a_field_kind_that_does_not_match_the_value_is_refused() {
    let mut policy = policy();
    policy.abstract_fields = vec![AbstractedField {
        field: "amount".into(),
        kind: VariableKind::Identifier,
    }];
    let error = abstract_payload(&payload(), &policy).unwrap_err();
    assert!(matches!(error, AbstractionError::FieldKindMismatch { .. }));
}

#[test]
fn an_unknown_field_in_the_release_is_refused() {
    let mut policy = policy();
    policy.abstract_fields.push(AbstractedField {
        field: "unknown".into(),
        kind: VariableKind::Text,
    });
    let error = abstract_payload(&payload(), &policy).unwrap_err();
    assert!(matches!(error, AbstractionError::MissingField { .. }));
}

#[test]
fn a_solution_with_an_unknown_variable_is_refused() {
    let abstraction = abstract_payload(&payload(), &policy()).unwrap();
    let solution = json!({"customer": {"$var": "v9"}});
    let error = reconstruct(&solution, &abstraction.bindings).unwrap_err();
    assert!(matches!(error, AbstractionError::UnknownVariable { .. }));
}

#[test]
fn a_number_variable_is_not_substituted_into_a_text() {
    let abstraction = abstract_payload(&payload(), &policy()).unwrap();
    let solution = json!({"note": "refund $v2 to the customer"});
    let error = reconstruct(&solution, &abstraction.bindings).unwrap_err();
    assert!(matches!(error, AbstractionError::UnresolvedVariable { .. }));
}

#[test]
fn a_text_variable_is_substituted_into_a_text() {
    let abstraction = abstract_payload(&payload(), &policy()).unwrap();
    let solution =
        json!({"note": "refund for $v1", "customer": {"$var": "v1"}, "sum": {"$var": "v2"}});
    let result = reconstruct(&solution, &abstraction.bindings).unwrap();
    assert_eq!(result["note"], json!("refund for C-4711"));
    assert_eq!(result["customer"], json!("C-4711"));
    assert_eq!(result["sum"], json!(12500));
}

#[test]
fn a_leftover_variable_is_refused() {
    let abstraction = abstract_payload(&payload(), &policy()).unwrap();
    let solution = json!({"note": "refund for $v1"});
    let result = reconstruct(&solution, &abstraction.bindings).unwrap();
    assert!(result["note"].as_str().unwrap().contains("C-4711"));
    // A variable that no binding belongs to is never invented.
    let error = reconstruct(&json!({"note": "$v7"}), &abstraction.bindings).unwrap_err();
    assert!(matches!(error, AbstractionError::UnknownVariable { .. }));
}

/// Erzeugt eine vertrauliche Quelle, aus der abstrahiert wird.
fn confidential_source(store: &SqliteArtifactStore, schemas: &SchemaRegistry) -> VersionId {
    let artifact = Artifact::new(
        "customer-request",
        "customer-request-v1",
        "customer_request",
        SOURCE_SCHEMA,
        "2026-11-28T00:00:00Z",
        Actor {
            actor_type: ActorType::Human,
            id: "user".into(),
        },
        Source {
            kind: SourceKind::Chat,
            reference: "customer".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::Human,
        },
        "customer-request",
        payload(),
    )
    .with_subject("customer-request")
    .with_security(SecurityProfile::declared(Classification::Confidential));
    store.append(&artifact, schemas).unwrap();
    artifact.version_id
}

fn ids(store: &SqliteArtifactStore) -> SequenceIdGenerator {
    store.durable_id_generator("privacy-test").unwrap()
}

#[test]
fn an_abstract_problem_is_released_while_the_source_stays_protected() {
    let schemas = schemas();
    let store = store();
    let time = Time {
        seconds: std::sync::atomic::AtomicU64::new(1_800_000_000),
    };
    let source = confidential_source(&store, &schemas);
    let mut generator = ids(&store);
    let mut service = AbstractionService::new(&store, &schemas, &mut generator, &time);

    let prepared = service
        .prepare(&source, &policy(), Classification::Internal)
        .unwrap();

    // The released problem carries no value and is classified like its target.
    let problem = store.get(&prepared.problem_version).unwrap().unwrap();
    assert_eq!(
        problem.security.effective(),
        Classification::Internal,
        "the released problem is classified by its target"
    );
    assert!(
        !serde_json::to_string(&problem.payload)
            .unwrap()
            .contains("C-4711")
    );

    // The binding stays with the source and is not released.
    let binding = store.get(&prepared.binding_version).unwrap().unwrap();
    assert_eq!(binding.security.effective(), Classification::Confidential);
    assert!(
        serde_json::to_string(&binding.payload)
            .unwrap()
            .contains("C-4711")
    );
}

#[test]
fn a_target_above_the_grant_ceiling_is_refused() {
    let schemas = schemas();
    let store = store();
    let time = Time {
        seconds: std::sync::atomic::AtomicU64::new(1_800_000_000),
    };
    let source = confidential_source(&store, &schemas);
    let mut generator = ids(&store);
    let mut service = AbstractionService::new(&store, &schemas, &mut generator, &time);

    let error = service
        .prepare(&source, &policy(), Classification::Public)
        .unwrap_err();
    assert!(matches!(
        error,
        AbstractionError::TargetClassExceedsGrant { .. }
    ));
}

#[test]
fn a_reconstructed_result_inherits_the_protection_of_its_source() {
    let schemas = schemas();
    let store = store();
    let time = Time {
        seconds: std::sync::atomic::AtomicU64::new(1_800_000_000),
    };
    let source = confidential_source(&store, &schemas);
    let mut generator = ids(&store);
    let mut service = AbstractionService::new(&store, &schemas, &mut generator, &time);
    let prepared = service
        .prepare(&source, &policy(), Classification::Internal)
        .unwrap();

    let solution = json!({
        "customer_id": {"$var": "v1"},
        "amount": {"$var": "v2"},
        "request": "please refund",
    });
    let result = service
        .reconstruct(
            &prepared,
            &solution,
            &crate::core::SchemaId(SOURCE_SCHEMA.into()),
        )
        .unwrap();
    assert_eq!(result.payload["customer_id"], json!("C-4711"));
    assert_eq!(
        result.security.effective(),
        Classification::Confidential,
        "the concrete solution is as sensitive as its cause"
    );
}

#[test]
fn an_incomplete_solution_leaves_no_result() {
    let schemas = schemas();
    let store = store();
    let time = Time {
        seconds: std::sync::atomic::AtomicU64::new(1_800_000_000),
    };
    let source = confidential_source(&store, &schemas);
    let mut generator = ids(&store);
    let mut service = AbstractionService::new(&store, &schemas, &mut generator, &time);
    let prepared = service
        .prepare(&source, &policy(), Classification::Internal)
        .unwrap();
    let before = store.len().unwrap();

    let solution = json!({"customer_id": {"$var": "v4"}});
    let error = service
        .reconstruct(
            &prepared,
            &solution,
            &crate::core::SchemaId(SOURCE_SCHEMA.into()),
        )
        .unwrap_err();
    assert!(matches!(error, AbstractionError::UnknownVariable { .. }));
    assert_eq!(
        store.len().unwrap(),
        before,
        "a failed reconstruction leaves no result behind"
    );
}

#[test]
fn an_abstraction_is_released_to_one_reasoner_only() {
    let payload = json!({"reasoner": reasoner()});
    assert!(ensure_authorized_reasoner(&payload, &reasoner()).is_ok());
    let other = CapabilityRef::new("peer", "other");
    let error = ensure_authorized_reasoner(&payload, &other).unwrap_err();
    assert!(matches!(
        error,
        AbstractionError::NotAuthorizedReasoner { .. }
    ));
}
