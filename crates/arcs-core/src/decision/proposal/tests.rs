use super::*;
use crate::adapters::{
    ADAPTER_PROTOCOL_VERSION, AdapterGrant, AdapterId, AdapterManifest, CapabilityContract,
    CapabilityDescriptor, CapabilityId, ProducerClass,
};
use crate::core::SystemClock;
use serde_json::json;

fn setup() -> (SchemaRegistry, SqliteArtifactStore, AdapterRegistry) {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(r#"{"$id":"arcs.rule_candidate.test.v1","type":"object","required":["enabled"],"properties":{"enabled":{"type":"boolean"}},"additionalProperties":false}"#).unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    store
        .append(
            &Artifact::new(
                "evidence-1",
                "evidence-1-v1",
                "input",
                "arcs.input.v1",
                "2026-10-22T12:00:00Z",
                Actor {
                    actor_type: ActorType::System,
                    id: "test.host".into(),
                },
                Source {
                    kind: SourceKind::Internal,
                    reference: "test.input".into(),
                },
                Trust {
                    level: TrustLevel::High,
                    source_class: SourceClass::System,
                },
                "test.input",
                json!({"raw_text":"evidence"}),
            ),
            &schemas,
        )
        .unwrap();
    let manifest = AdapterManifest {
        protocol_version: ADAPTER_PROTOCOL_VERSION,
        adapter_id: AdapterId("test.device".into()),
        adapter_version: "1.0.0".into(),
        capabilities: vec![CapabilityDescriptor {
            id: CapabilityId("set".into()),
            contract: CapabilityContract::Act {
                accepts: vec![SchemaId("arcs.rule_candidate.test.v1".into())],
                emits: vec![SchemaId("arcs.execution_result.v1".into())],
                idempotent: true,
            },
            required_permissions: vec!["write".into()],
        }],
    };
    let grant = AdapterGrant {
        adapter_id: manifest.adapter_id.clone(),
        producer_class: ProducerClass::Executor,
        enabled_capabilities: vec![CapabilityId("set".into())],
        granted_permissions: vec!["write".into()],
        assigned_trust: TrustLevel::Medium,
        ingress_source_kind: None,
        max_payload_bytes: 4096,
        max_external_reference_bytes: 512,
        data_limits: None,
        input_data_limits: None,
        reasoning_limits_v2: None,
        reasoning_limits: None,
    };
    let mut registry = AdapterRegistry::new();
    registry
        .register(manifest, grant, &schemas, &store)
        .unwrap();
    (schemas, store, registry)
}

fn proposal() -> CandidateProposal {
    CandidateProposal {
        proposal_id: "proposal-1".into(),
        rule_id: "test.rule.v1".into(),
        schema_id: SchemaId("arcs.rule_candidate.test.v1".into()),
        payload: json!({"enabled": true}),
        evidence: vec![VersionId("evidence-1-v1".into())],
        required_capabilities: vec![CapabilityRef::new("test.device", "set")],
    }
}

#[test]
fn deterministic_proposal_is_auditable_idempotent_and_has_no_action_authority() {
    let (schemas, store, registry) = setup();
    let mut ids = store.durable_id_generator("test").unwrap();
    let mut service = ProposalService::new(&registry, &schemas, &store, &mut ids, &SystemClock);
    let first = service.propose(proposal()).unwrap();
    assert_eq!(first, service.propose(proposal()).unwrap());
    assert_eq!(store.len().unwrap(), 2);
    assert_eq!(first.artifact_type, "rule_candidate");
    assert_eq!(first.created_by.actor_type, ActorType::System);
    let provenance = first.provenance.as_ref().unwrap();
    assert!(provenance.models_used.is_empty());
    assert!(provenance.rules_applied.contains(&"test.rule.v1".into()));
    assert_eq!(provenance.parents, ["evidence-1-v1"]);
    assert_eq!(
        store.outgoing_relations(&first.version_id).unwrap()[0].kind,
        relation_kinds::supported_by()
    );
    assert_eq!(
        store
            .committed_after(0)
            .unwrap()
            .iter()
            .filter(|c| c.artifact.artifact_type == "action")
            .count(),
        0
    );
    let mut changed = proposal();
    changed.rule_id = "test.rule.v2".into();
    assert!(matches!(
        service.propose(changed),
        Err(ProposalError::Store(StoreError::IdempotencyConflict))
    ));
    assert_eq!(store.len().unwrap(), 2);
}

#[test]
fn uncommitted_evidence_and_disabled_capabilities_are_rejected_before_append() {
    let (schemas, store, registry) = setup();
    let mut ids = store.durable_id_generator("test").unwrap();
    let mut service = ProposalService::new(&registry, &schemas, &store, &mut ids, &SystemClock);
    let mut missing = proposal();
    missing.evidence = vec![VersionId("missing-version".into())];
    assert!(matches!(
        service.propose(missing),
        Err(ProposalError::MissingEvidence(_))
    ));
    let mut forbidden = proposal();
    forbidden.required_capabilities = vec![CapabilityRef::new("test.device", "not-enabled")];
    assert!(matches!(
        service.propose(forbidden),
        Err(ProposalError::Authorization(_))
    ));
    assert_eq!(store.len().unwrap(), 1);
}

#[test]
fn host_proposal_cannot_masquerade_as_an_action_artifact() {
    let (schemas, store, registry) = setup();
    let mut ids = store.durable_id_generator("test").unwrap();
    let mut request = proposal();
    request.schema_id = SchemaId("arcs.action.v1".into());
    let result =
        ProposalService::new(&registry, &schemas, &store, &mut ids, &SystemClock).propose(request);
    assert!(matches!(result, Err(ProposalError::NotCandidateSchema)));
    assert_eq!(store.len().unwrap(), 1);
}
