use crate::core::VersionId;
use crate::store::{
    ActivatedArtifact, NetworkEdge, NetworkNeighbor, SqliteArtifactStore, StoreError,
};

pub struct ArtifactNetwork<'a> {
    store: &'a SqliteArtifactStore,
}

impl<'a> ArtifactNetwork<'a> {
    pub fn new(store: &'a SqliteArtifactStore) -> Self {
        Self { store }
    }

    pub fn connect(&self, from: VersionId, to: VersionId, weight: f64) -> Result<(), StoreError> {
        self.store.connect(&NetworkEdge { from, to, weight })
    }

    pub fn neighbors(&self, source: &VersionId) -> Result<Vec<NetworkNeighbor>, StoreError> {
        let edges = self.store.outgoing_edges(source)?;
        let mut neighbors = Vec::new();

        for edge in edges {
            if let Some(artifact) = self.store.get(&edge.to)? {
                neighbors.push(NetworkNeighbor {
                    artifact,
                    weight: edge.weight,
                });
            }
        }

        Ok(neighbors)
    }

    /// Leitet eine Aktivierung genau einen Schritt über ausgehende Kanten weiter.
    ///
    /// Das Ergebnis ist flüchtig und wird nicht in SQLite gespeichert.
    pub fn propagate_once(
        &self,
        source: &VersionId,
        source_activation: f64,
    ) -> Result<Vec<ActivatedArtifact>, StoreError> {
        let neighbors = self.neighbors(source)?;
        Ok(neighbors
            .into_iter()
            .map(|neighbor| ActivatedArtifact {
                activation: source_activation * neighbor.weight,
                via_weight: neighbor.weight,
                artifact: neighbor.artifact,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::core::{
        Actor, ActorType, Artifact, SchemaRegistry, Source, SourceClass, SourceKind, Trust,
        TrustLevel,
    };

    use super::*;

    fn input(artifact_id: &str) -> Artifact {
        Artifact::new(
            artifact_id,
            format!("{artifact_id}-v1"),
            "input",
            "arcs.input.v1",
            "2026-07-26T23:00:00+02:00",
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
            "input:1",
            json!({"raw_text": artifact_id}),
        )
    }

    #[test]
    fn resolves_connected_versions_to_neighbor_artifacts() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let source = input("source");
        let target = input("target");
        store.append(&source, &registry).unwrap();
        store.append(&target, &registry).unwrap();

        let network = ArtifactNetwork::new(&store);
        network
            .connect(source.version_id.clone(), target.version_id.clone(), 0.9)
            .unwrap();

        assert_eq!(
            network.neighbors(&source.version_id).unwrap(),
            vec![NetworkNeighbor {
                artifact: target,
                weight: 0.9,
            }]
        );
        assert!(
            network
                .neighbors(&VersionId("unknown-v1".into()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn propagates_activation_across_persisted_edges() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let source = input("source");
        let activating_target = input("activating-target");
        let inhibiting_target = input("inhibiting-target");
        for artifact in [&source, &activating_target, &inhibiting_target] {
            store.append(artifact, &registry).unwrap();
        }

        let network = ArtifactNetwork::new(&store);
        network
            .connect(
                source.version_id.clone(),
                activating_target.version_id.clone(),
                0.8,
            )
            .unwrap();
        network
            .connect(
                source.version_id.clone(),
                inhibiting_target.version_id.clone(),
                -0.25,
            )
            .unwrap();

        assert_eq!(
            network.propagate_once(&source.version_id, 0.5).unwrap(),
            vec![
                ActivatedArtifact {
                    artifact: activating_target,
                    activation: 0.4,
                    via_weight: 0.8,
                },
                ActivatedArtifact {
                    artifact: inhibiting_target,
                    activation: -0.125,
                    via_weight: -0.25,
                },
            ]
        );
    }
}
