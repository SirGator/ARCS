use serde_json::json;

use super::*;
use crate::core::{
    Actor, ActorType, Artifact, SchemaRegistry, Source, SourceClass, SourceKind, Trust, TrustLevel,
};
use crate::store::relation_kinds;

fn input(id: &str) -> Artifact {
    Artifact::new(
        id,
        format!("{id}-v1"),
        "input",
        "arcs.input.v1",
        "2026-07-27T12:00:00Z",
        Actor {
            actor_type: ActorType::System,
            id: "relation-test".into(),
        },
        Source {
            kind: SourceKind::Internal,
            reference: "test".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        "relations:test",
        json!({"raw_text": id}),
    )
}

#[test]
fn persists_typed_relation_without_creating_activation_edge() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let request = input("request");
    let observation = input("observation");
    for artifact in [&request, &observation] {
        store.append(artifact, &schemas).unwrap();
    }

    let relations = ArtifactRelations::new(&store);
    relations
        .connect(
            observation.version_id.clone(),
            request.version_id.clone(),
            relation_kinds::fulfills(),
        )
        .unwrap();

    assert_eq!(
        relations.outgoing(&observation.version_id).unwrap(),
        vec![ArtifactRelation {
            from: observation.version_id.clone(),
            to: request.version_id,
            kind: relation_kinds::fulfills(),
        }]
    );
    assert!(
        crate::store::ArtifactNetwork::new(&store)
            .neighbors(&observation.version_id)
            .unwrap()
            .is_empty()
    );
}
