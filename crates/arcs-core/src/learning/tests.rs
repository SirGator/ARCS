use serde_json::json;

use super::{LearningError, LearningPolicy, LearningService};
use crate::core::{
    Actor, ActorType, Artifact, SchemaRegistry, Source, SourceClass, SourceKind, Trust, TrustLevel,
};
use crate::store::{ActiveSource, ArtifactNetwork, NetworkError, SqliteArtifactStore};

fn artifact(id: &str) -> Artifact {
    Artifact::new(
        id,
        format!("{id}-v1"),
        "input",
        "arcs.input.v1",
        "2026-08-08T12:00:00Z",
        Actor {
            actor_type: ActorType::Human,
            id: "user-1".into(),
        },
        Source {
            kind: SourceKind::Chat,
            reference: format!("chat://{id}"),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::Human,
        },
        format!("input:{id}"),
        json!({"raw_text": id}),
    )
}

#[test]
fn successful_relation_is_strengthened() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let source = artifact("source");
    let target = artifact("target");
    for artifact in [&source, &target] {
        store.append(artifact, &registry).unwrap();
    }

    let network = ArtifactNetwork::new(&store);
    network
        .connect(source.version_id.clone(), target.version_id.clone(), 0.40)
        .unwrap();
    let learning = LearningService::new(&network, LearningPolicy::default());

    let new_weight = learning
        .reinforce(&source.version_id, &target.version_id)
        .unwrap();

    assert!((new_weight - 0.45).abs() < f64::EPSILON);
    let edge = network
        .edge(&source.version_id, &target.version_id)
        .unwrap()
        .expect("the reinforced relation should still exist");
    assert!((edge.weight - 0.45).abs() < f64::EPSILON);
}

#[test]
fn reinforcement_is_clamped_at_maximum() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let source = artifact("source");
    let target = artifact("target");
    for artifact in [&source, &target] {
        store.append(artifact, &registry).unwrap();
    }

    let network = ArtifactNetwork::new(&store);
    network
        .connect(source.version_id.clone(), target.version_id.clone(), 0.98)
        .unwrap();
    let learning = LearningService::new(&network, LearningPolicy::default());

    let new_weight = learning
        .reinforce(&source.version_id, &target.version_id)
        .unwrap();

    assert_eq!(new_weight, 1.0);
    assert_eq!(
        network
            .edge(&source.version_id, &target.version_id)
            .unwrap()
            .expect("the reinforced relation should still exist")
            .weight,
        1.0
    );
}

#[test]
fn missing_relation_cannot_be_reinforced() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let source = artifact("source");
    let target = artifact("target");
    for artifact in [&source, &target] {
        store.append(artifact, &registry).unwrap();
    }

    let network = ArtifactNetwork::new(&store);
    let learning = LearningService::new(&network, LearningPolicy::default());

    assert!(matches!(
        learning.reinforce(&source.version_id, &target.version_id),
        Err(LearningError::Network(NetworkError::MissingEdge { from, to }))
            if from == source.version_id && to == target.version_id
    ));
}

#[test]
fn reinforcement_changes_future_routing_strength() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let input = artifact("input");
    let target_b = artifact("target-b");
    let target_c = artifact("target-c");
    for artifact in [&input, &target_b, &target_c] {
        store.append(artifact, &registry).unwrap();
    }

    let network = ArtifactNetwork::new(&store);
    network
        .connect(input.version_id.clone(), target_b.version_id.clone(), 0.50)
        .unwrap();
    network
        .connect(input.version_id.clone(), target_c.version_id.clone(), 0.55)
        .unwrap();
    let source = [ActiveSource {
        version_id: input.version_id.clone(),
        activation: 1.0,
    }];

    let before_learning = network.propagate_many(&source, 0.0).unwrap();
    assert_eq!(before_learning[0].artifact.version_id, target_c.version_id);

    let learning = LearningService::new(&network, LearningPolicy::default());
    learning
        .reinforce(&input.version_id, &target_b.version_id)
        .unwrap();
    let reinforced_weight = learning
        .reinforce(&input.version_id, &target_b.version_id)
        .unwrap();
    assert!((reinforced_weight - 0.60).abs() < 0.000_001);

    let after_learning = network.propagate_many(&source, 0.0).unwrap();
    assert_eq!(after_learning[0].artifact.version_id, target_b.version_id);
    assert!((after_learning[0].activation - 0.60).abs() < 0.000_001);
    assert_eq!(after_learning[1].artifact.version_id, target_c.version_id);
    assert!((after_learning[1].activation - 0.55).abs() < 0.000_001);
}
