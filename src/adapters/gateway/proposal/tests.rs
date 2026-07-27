use serde_json::json;

use super::*;
use crate::adapters::{AdapterId, ArtifactIdGenerator, GeneratedArtifactIds, ReasoningTrace};
use crate::core::{Actor, ArtifactId, SchemaId, SchemaRegistry, Source, VersionId};
use crate::store::{
    ArtifactNetwork, ArtifactRelation, ArtifactRelations, SqliteArtifactStore, relation_kinds,
};

const ROUTE_SCHEMA: &str = r#"{
    "$id": "arcs.route_candidate.commit_test.v1",
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "type": "object",
    "required": ["summary"],
    "properties": {
        "summary": {"type": "string", "minLength": 1}
    },
    "additionalProperties": false
}"#;

struct FixedClock;

impl crate::adapters::Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-07-27T12:00:00Z".into()
    }
}

struct TestIds;

impl ArtifactIdGenerator for TestIds {
    fn next(&mut self, artifact_type: &str) -> GeneratedArtifactIds {
        GeneratedArtifactIds {
            artifact_id: ArtifactId(format!("{artifact_type}-1")),
            version_id: VersionId(format!("{artifact_type}-1-v1")),
        }
    }
}

fn input_artifact() -> Artifact {
    Artifact::new(
        "input-1",
        "input-1-v1",
        "input",
        "arcs.input.v1",
        "2026-07-27T11:59:00Z",
        Actor {
            actor_type: ActorType::Human,
            id: "user-1".into(),
        },
        Source {
            kind: SourceKind::Chat,
            reference: "chat-1".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::Human,
        },
        "conversation-1",
        json!({"raw_text": "Turn on the light"}),
    )
}

fn reasoning_request_artifact(context: &VersionId) -> Artifact {
    Artifact::new(
        "reasoning-request-1",
        "reasoning-request-1-v1",
        "reasoning_request",
        "arcs.reasoning_request.v1",
        "2026-07-27T12:00:00Z",
        Actor {
            actor_type: ActorType::System,
            id: "arcs.reasoning_gateway".into(),
        },
        Source {
            kind: SourceKind::Internal,
            reference: "reasoning-request:reason-1".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        "reasoning:reason-1",
        json!({
            "objective": "Choose a safe route",
            "context_refs": [context.0.clone()]
        }),
    )
}

fn proposal(context: VersionId, reasoning_request: VersionId) -> ValidatedProposal {
    ValidatedProposal {
        adapter_id: AdapterId("reasoning.mock".into()),
        request_id: "reason-1".into(),
        reasoning_request_version: reasoning_request,
        candidate_index: 0,
        schema_id: SchemaId("arcs.route_candidate.commit_test.v1".into()),
        required_capabilities: vec![],
        referenced_versions: vec![context.clone()],
        context_versions: vec![context],
        payload: json!({"summary": "Ask a policy layer to evaluate this route"}),
        trace: ReasoningTrace {
            model_name: "mock-model".into(),
            prompt_hash: "prompt-sha256".into(),
            raw_output_hash: "output-sha256".into(),
            temperature: 0.0,
        },
    }
}

#[test]
fn commits_candidate_as_low_trust_once_without_creating_network_edge() {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas.register_json(ROUTE_SCHEMA).unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = input_artifact();
    store.append(&input, &schemas).unwrap();
    let reasoning_request = reasoning_request_artifact(&input.version_id);
    store.append(&reasoning_request, &schemas).unwrap();
    let mut gateway = AdapterGateway::new(
        &mut schemas,
        &store,
        Box::new(FixedClock),
        Box::new(TestIds),
    );
    let proposal = proposal(
        input.version_id.clone(),
        reasoning_request.version_id.clone(),
    );

    let committed = gateway.commit_proposal(proposal.clone()).unwrap();

    assert_eq!(store.len().unwrap(), 3);
    assert_eq!(committed.created_by.actor_type, ActorType::Model);
    assert_eq!(committed.trust.level, TrustLevel::Low);
    assert_eq!(committed.trust.source_class, SourceClass::Model);
    assert_eq!(
        committed.provenance.as_ref().unwrap().models_used[0].inputs,
        vec![input.version_id.0.clone()]
    );
    let network = ArtifactNetwork::new(&store);
    assert!(network.neighbors(&input.version_id).unwrap().is_empty());
    let semantic_relations = ArtifactRelations::new(&store)
        .outgoing(&committed.version_id)
        .unwrap();
    assert!(semantic_relations.contains(&ArtifactRelation {
        from: committed.version_id.clone(),
        to: input.version_id.clone(),
        kind: relation_kinds::supported_by(),
    }));
    assert!(semantic_relations.contains(&ArtifactRelation {
        from: committed.version_id.clone(),
        to: reasoning_request.version_id,
        kind: relation_kinds::generated_by(),
    }));

    let replay = gateway.commit_proposal(proposal);
    assert!(matches!(
        replay,
        Err(AdapterGatewayError::ProposalAlreadyCommitted {
            adapter,
            request_id,
            candidate_index: 0,
        }) if adapter == AdapterId("reasoning.mock".into()) && request_id == "reason-1"
    ));
    assert_eq!(store.len().unwrap(), 3);
}
