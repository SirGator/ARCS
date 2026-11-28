//! Nachweis: ein vertraulicher Sachverhalt wird extern nur abstrakt bearbeitet.
//!
//! Der Schnitt ist derselbe wie in der Produktion: Der Core baut die
//! Abstraktion, der externe Reasoner sieht nur Struktur, und die konkrete
//! Lösung entsteht lokal. Beide negativen Fälle stehen daneben: Der direkte
//! Weg mit vertraulichem Kontext wird abgewiesen, und eine Abstraktion wird
//! nicht an einen fremden Reasoner freigegeben.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterCallError, AdapterGrant, AdapterId, AdapterManifest,
    AdapterRegistry, CapabilityContract, CapabilityDescriptor, CapabilityId, CapabilityRef,
    ProducerClass,
};
use crate::core::{
    Actor, ActorType, Artifact, ArtifactId, ArtifactIdGenerator, Classification, Clock,
    GeneratedArtifactIds, SchemaId, SchemaRegistry, SecurityProfile, Source, SourceClass,
    SourceKind, Trust, TrustLevel, VersionId,
};
use crate::privacy::{
    ABSTRACT_PROBLEM_SCHEMA, AbstractedField, AbstractionPolicy, AbstractionService, VariableKind,
};
use crate::store::SqliteArtifactStore;

const SOURCE_SCHEMA: &str = r#"{
    "$id": "arcs.customer_request.abstraction_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["customer_id", "amount", "request"],
    "properties": {
        "customer_id": {"type": "string"},
        "amount": {"type": "number"},
        "request": {"type": "string"}
    },
    "additionalProperties": false
}"#;

/// Vertrag eines abstrakten Kandidaten.
///
/// Die Platzhalter sind bewusst untypisiert: Der Vorschlag entsteht ohne den
/// konkreten Wert, seine Form kennt der Core erst bei der Rekonstruktion.
const CANDIDATE_SCHEMA: &str = r#"{
    "$id": "arcs.abstract_candidate.abstraction_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["customer_id", "amount", "request"],
    "properties": {
        "customer_id": {},
        "amount": {},
        "request": {"type": "string"}
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

/// Externer Reasoner, der sich merkt, was er gesehen hat.
struct RecordingReasoner {
    manifest: AdapterManifest,
    answer: Value,
    seen: Arc<Mutex<Vec<ReasoningInvocation>>>,
}

impl ReasoningAdapter for RecordingReasoner {
    fn manifest(&self) -> &AdapterManifest {
        &self.manifest
    }

    fn propose(
        &self,
        request: &ReasoningInvocation,
    ) -> Result<ReasoningResponse, AdapterCallError> {
        self.seen.lock().unwrap().push(request.clone());
        Ok(ReasoningResponse {
            invocation_id: request.invocation_id.clone(),
            request_id: request.request_id.clone(),
            candidates: vec![ProposalSubmission {
                schema_id: request.target_schema_id.clone(),
                required_capabilities: vec![],
                referenced_versions: vec![],
                payload: self.answer.clone(),
            }],
            trace: ReasoningTrace {
                model_name: "external-reasoner".into(),
                prompt_hash: "prompt-sha256".into(),
                raw_output_hash: "output-sha256".into(),
                temperature: 0.0,
            },
        })
    }
}

fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(SOURCE_SCHEMA).unwrap();
    schemas.register_json(CANDIDATE_SCHEMA).unwrap();
    schemas
}

fn reasoning_manifest() -> AdapterManifest {
    AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("reasoning.test".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            description: None,
            id: CapabilityId("reasoning.propose".into()),
            contract: CapabilityContract::Reason {
                emits: vec![SchemaId(
                    "arcs.abstract_candidate.abstraction_test.v1".into(),
                )],
            },
            required_permissions: vec![],
        }],
    }
}

fn grant(ceiling: Classification) -> AdapterGrant {
    AdapterGrant {
        boundary: None,
        adapter_id: AdapterId("reasoning.test".into()),
        producer_class: ProducerClass::Model,
        enabled_capabilities: vec![CapabilityId("reasoning.propose".into())],
        granted_permissions: vec![],
        assigned_trust: TrustLevel::Low,
        ingress_source_kind: None,
        max_payload_bytes: 8192,
        max_external_reference_bytes: 512,
        data_limits: None,
        input_data_limits: None,
        reasoning_limits_v2: None,
        reasoning_limits: Some(ReasoningLimits {
            max_context_items: 4,
            max_context_bytes: 8192,
            max_output_tokens: 512,
            max_output_bytes: 8192,
            max_candidates: 2,
        }),
        data_classification: Some(ceiling),
    }
}

fn source() -> Artifact {
    Artifact::new(
        "customer-request",
        "customer-request-v1",
        "customer_request",
        "arcs.customer_request.abstraction_test.v1",
        "2026-11-28T08:00:00Z",
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
        "customer-request",
        json!({
            "customer_id": "C-4711",
            "amount": 12500,
            "request": "please refund",
        }),
    )
    .with_subject("customer-request")
    .with_security(SecurityProfile::declared(Classification::Confidential))
}

fn policy() -> AbstractionPolicy {
    AbstractionPolicy {
        purpose: "resolve customer request".into(),
        target_classification: Classification::Internal,
        reasoner: CapabilityRef::new("reasoning.test", "reasoning.propose"),
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

fn request(context: Vec<ContextSelection>) -> ReasoningRequest {
    ReasoningRequest {
        request_id: "abstraction-1".into(),
        reasoning_capability: CapabilityRef::new("reasoning.test", "reasoning.propose"),
        objective: "resolve the customer request".into(),
        context,
        target_schema_id: SchemaId("arcs.abstract_candidate.abstraction_test.v1".into()),
        allowed_capabilities: vec![],
        constraints: json!({}),
        budget: ReasoningBudget {
            max_context_items: 2,
            max_context_bytes: 8192,
            max_output_tokens: 256,
            max_output_bytes: 8192,
            max_candidates: 1,
        },
    }
}

#[test]
fn a_confidential_context_is_refused_at_the_disclosure_edge() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let artifact = source();
    store.append(&artifact, &schemas).unwrap();
    let adapter_manifest = reasoning_manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(
            adapter_manifest.clone(),
            grant(Classification::Internal),
            &schemas,
            &store,
        )
        .unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let endpoint = RecordingReasoner {
        manifest: adapter_manifest,
        answer: json!({"customer_id": {}, "amount": {}, "request": "refund"}),
        seen: Arc::clone(&seen),
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

    let error = service
        .reason(request(vec![ContextSelection {
            version_id: artifact.version_id.clone(),
            payload_fields: vec!["customer_id".into()],
        }]))
        .unwrap_err();
    assert!(
        matches!(error, ReasoningError::DataClassExceedsGrant { .. }),
        "the confidential context must not reach the external reasoner: {error:?}"
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "the reasoner must not have been called at all"
    );
}

#[test]
fn an_abstraction_is_solved_externally_and_reconstructed_locally() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let artifact = source();
    store.append(&artifact, &schemas).unwrap();
    let adapter_manifest = reasoning_manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(
            adapter_manifest.clone(),
            grant(Classification::Internal),
            &schemas,
            &store,
        )
        .unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let endpoint = RecordingReasoner {
        manifest: adapter_manifest,
        answer: json!({
            "customer_id": {"$var": "v1"},
            "amount": {"$var": "v2"},
            "request": "please refund",
        }),
        seen: Arc::clone(&seen),
    };

    let mut ids = TestIds(1);
    let mut abstraction = AbstractionService::new(&store, &schemas, &mut ids, &FixedClock);
    let prepared = abstraction
        .prepare(&artifact.version_id, &policy(), Classification::Internal)
        .unwrap();

    let mut reasoning_ids = TestIds(500);
    let mut service = ReasoningService::new(
        &registry,
        &schemas,
        &store,
        &mut reasoning_ids,
        &FixedClock,
        &endpoint,
    );
    let proposals = service
        .reason(request(vec![ContextSelection {
            version_id: prepared.problem_version.clone(),
            payload_fields: vec!["problem".into(), "variables".into()],
        }]))
        .unwrap();

    // The external reasoner saw the structure but never a value.
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let wire = serde_json::to_string(&seen[0]).unwrap();
    assert!(
        !wire.contains("C-4711") && !wire.contains("12500"),
        "no confidential value may leave the core: {wire}"
    );
    assert!(wire.contains("\"$var\":\"v1\""));

    // The concrete answer is reconstructed locally and inherits the source.
    let proposal = proposals.into_iter().next().unwrap();
    let result = abstraction
        .reconstruct(
            &prepared,
            &proposal.payload,
            &SchemaId("arcs.customer_request.abstraction_test.v1".into()),
        )
        .unwrap();
    assert_eq!(result.payload["customer_id"], json!("C-4711"));
    assert_eq!(result.payload["amount"], json!(12500));
    assert_eq!(
        result.security.effective(),
        Classification::Confidential,
        "the reconstructed answer is as sensitive as the request that caused it"
    );
}

#[test]
fn an_abstraction_is_not_released_to_another_reasoner() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let artifact = source();
    store.append(&artifact, &schemas).unwrap();
    let adapter_manifest = reasoning_manifest();
    let mut registry = AdapterRegistry::new();
    registry
        .register(
            adapter_manifest.clone(),
            grant(Classification::Internal),
            &schemas,
            &store,
        )
        .unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let endpoint = RecordingReasoner {
        manifest: adapter_manifest,
        answer: json!({"customer_id": {}, "amount": {}, "request": "refund"}),
        seen: Arc::clone(&seen),
    };
    let mut ids = TestIds(1);
    let mut abstraction = AbstractionService::new(&store, &schemas, &mut ids, &FixedClock);

    // The operator released the abstraction to another capability of the
    // same installation.
    let mut foreign_policy = policy();
    foreign_policy.reasoner = CapabilityRef::new("reasoning.test", "reasoning.hidden");
    let prepared = abstraction
        .prepare(
            &artifact.version_id,
            &foreign_policy,
            Classification::Internal,
        )
        .unwrap();

    let mut reasoning_ids = TestIds(500);
    let mut service = ReasoningService::new(
        &registry,
        &schemas,
        &store,
        &mut reasoning_ids,
        &FixedClock,
        &endpoint,
    );
    let error = service
        .reason(request(vec![ContextSelection {
            version_id: prepared.problem_version.clone(),
            payload_fields: vec!["problem".into()],
        }]))
        .unwrap_err();
    assert!(
        matches!(error, ReasoningError::InvalidReasoningRequest(_)),
        "the abstraction belongs to one reasoner only: {error:?}"
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "the reasoner must not have been called"
    );
}

#[test]
fn a_released_problem_stays_classified_at_its_target() {
    let schemas = schemas();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let artifact = source();
    store.append(&artifact, &schemas).unwrap();
    let mut ids = TestIds(1);
    let mut abstraction = AbstractionService::new(&store, &schemas, &mut ids, &FixedClock);
    let prepared = abstraction
        .prepare(&artifact.version_id, &policy(), Classification::Internal)
        .unwrap();

    let released = store.get(&prepared.problem_version).unwrap().unwrap();
    assert_eq!(released.schema_id.0, ABSTRACT_PROBLEM_SCHEMA);
    assert_eq!(released.security.effective(), Classification::Internal);
    assert!(
        released.security.declassification_reason.is_some(),
        "the downgrade carries a documented reason"
    );
}
