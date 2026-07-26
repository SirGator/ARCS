use std::collections::{HashMap, HashSet};

use crate::core::VersionId;
use crate::store::{
    ActivatedArtifact, ActiveSource, NetworkEdge, NetworkNeighbor, SqliteArtifactStore, StoreError,
};

/// Fehler an der fachlichen Grenze des Artefaktnetzes.
#[derive(Debug)]
pub enum NetworkError {
    /// Die technische Persistenzoperation ist fehlgeschlagen.
    Store(StoreError),
    /// Quellaktivierungen müssen endlich und auf `0.0..=1.0` normiert sein.
    InvalidActivation(f64),
    /// Der Schwellwert muss endlich und größer oder gleich null sein.
    InvalidThreshold(f64),
    /// Dieselbe Quellversion darf in einem Aufruf nicht mehrfach beitragen.
    DuplicateSource(VersionId),
}

impl From<StoreError> for NetworkError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

pub struct ArtifactNetwork<'a> {
    store: &'a SqliteArtifactStore,
}

impl<'a> ArtifactNetwork<'a> {
    pub fn new(store: &'a SqliteArtifactStore) -> Self {
        Self { store }
    }

    pub fn connect(&self, from: VersionId, to: VersionId, weight: f64) -> Result<(), NetworkError> {
        self.store.connect(&NetworkEdge { from, to, weight })?;
        Ok(())
    }

    pub fn neighbors(&self, source: &VersionId) -> Result<Vec<NetworkNeighbor>, NetworkError> {
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
    ) -> Result<Vec<ActivatedArtifact>, NetworkError> {
        validate_activation(source_activation)?;

        let neighbors = self.neighbors(source)?;
        Ok(neighbors
            .into_iter()
            .map(|neighbor| ActivatedArtifact {
                artifact: neighbor.artifact,
                activation: source_activation * neighbor.weight,
            })
            .collect())
    }

    /// Aggregiert mehrere direkte Quellbeiträge und wählt aktivierte Ziele aus.
    ///
    /// Ein Ziel wird nur zurückgegeben, wenn die Summe seiner gewichteten
    /// Beiträge mindestens `threshold` erreicht. Die Ergebnisse sind absteigend
    /// nach Aktivierung sortiert. Es wird keine Aktivierung persistiert.
    pub fn propagate_many(
        &self,
        sources: &[ActiveSource],
        threshold: f64,
    ) -> Result<Vec<ActivatedArtifact>, NetworkError> {
        if !threshold.is_finite() || threshold < 0.0 {
            return Err(NetworkError::InvalidThreshold(threshold));
        }

        let mut seen_sources = HashSet::new();
        let mut target_activations = HashMap::<VersionId, f64>::new();

        for source in sources {
            validate_activation(source.activation)?;
            if !seen_sources.insert(source.version_id.clone()) {
                return Err(NetworkError::DuplicateSource(source.version_id.clone()));
            }

            for edge in self.store.outgoing_edges(&source.version_id)? {
                let activation = target_activations.entry(edge.to).or_insert(0.0);
                *activation += source.activation * edge.weight;
                if !activation.is_finite() {
                    return Err(NetworkError::InvalidActivation(*activation));
                }
            }
        }

        let mut results = Vec::new();
        for (version_id, activation) in target_activations {
            if activation < threshold {
                continue;
            }
            if let Some(artifact) = self.store.get(&version_id)? {
                results.push(ActivatedArtifact {
                    artifact,
                    activation,
                });
            }
        }

        results.sort_by(|a, b| {
            b.activation
                .total_cmp(&a.activation)
                .then_with(|| a.artifact.version_id.0.cmp(&b.artifact.version_id.0))
        });
        Ok(results)
    }
}

fn validate_activation(activation: f64) -> Result<(), NetworkError> {
    if !activation.is_finite() || !(0.0..=1.0).contains(&activation) {
        return Err(NetworkError::InvalidActivation(activation));
    }
    Ok(())
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
                },
                ActivatedArtifact {
                    artifact: inhibiting_target,
                    activation: -0.125,
                },
            ]
        );
    }

    #[test]
    fn only_the_combination_of_input_and_state_activates_target() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let input_artifact = input("input");
        let state = input("state");
        let target = input("target");
        for artifact in [&input_artifact, &state, &target] {
            store.append(artifact, &registry).unwrap();
        }

        let network = ArtifactNetwork::new(&store);
        network
            .connect(
                input_artifact.version_id.clone(),
                target.version_id.clone(),
                0.7,
            )
            .unwrap();
        network
            .connect(state.version_id.clone(), target.version_id.clone(), 0.5)
            .unwrap();
        let input_only = [ActiveSource {
            version_id: input_artifact.version_id.clone(),
            activation: 0.6,
        }];
        let state_only = [ActiveSource {
            version_id: state.version_id.clone(),
            activation: 0.8,
        }];
        let combined = [
            ActiveSource {
                version_id: input_artifact.version_id,
                activation: 0.6,
            },
            ActiveSource {
                version_id: state.version_id,
                activation: 0.8,
            },
        ];

        assert!(
            network
                .propagate_many(&input_only, 0.75)
                .unwrap()
                .is_empty()
        );
        assert!(
            network
                .propagate_many(&state_only, 0.75)
                .unwrap()
                .is_empty()
        );

        let activated = network.propagate_many(&combined, 0.75).unwrap();
        assert_eq!(activated.len(), 1);
        assert_eq!(activated[0].artifact.version_id, target.version_id);
        assert!((activated[0].activation - 0.82).abs() < 0.000_001);
    }

    #[test]
    fn returns_activated_targets_from_strongest_to_weakest() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let source = input("source");
        let weaker_target = input("weaker-target");
        let stronger_target = input("stronger-target");
        for artifact in [&source, &weaker_target, &stronger_target] {
            store.append(artifact, &registry).unwrap();
        }

        let network = ArtifactNetwork::new(&store);
        network
            .connect(
                source.version_id.clone(),
                weaker_target.version_id.clone(),
                0.4,
            )
            .unwrap();
        network
            .connect(
                source.version_id.clone(),
                stronger_target.version_id.clone(),
                0.9,
            )
            .unwrap();

        let activated = network
            .propagate_many(
                &[ActiveSource {
                    version_id: source.version_id,
                    activation: 1.0,
                }],
                0.1,
            )
            .unwrap();

        assert_eq!(activated.len(), 2);
        assert_eq!(activated[0].artifact, stronger_target);
        assert_eq!(activated[1].artifact, weaker_target);
    }

    #[test]
    fn rejects_invalid_or_duplicate_sources() {
        let store = SqliteArtifactStore::in_memory().unwrap();
        let network = ArtifactNetwork::new(&store);
        let source = VersionId("source-v1".into());

        assert!(matches!(
            network.propagate_many(&[], f64::NAN),
            Err(NetworkError::InvalidThreshold(value)) if value.is_nan()
        ));
        assert!(matches!(
            network.propagate_many(&[], -0.1),
            Err(NetworkError::InvalidThreshold(value)) if value == -0.1
        ));

        for activation in [f64::INFINITY, f64::NAN, -0.1, 1.1] {
            assert!(matches!(
                network.propagate_many(
                    &[ActiveSource {
                        version_id: source.clone(),
                        activation,
                    }],
                    0.75,
                ),
                Err(NetworkError::InvalidActivation(value))
                    if value.is_nan() || value == activation
            ));
        }

        assert!(matches!(
            network.propagate_many(
                &[
                    ActiveSource {
                        version_id: source.clone(),
                        activation: 0.6,
                    },
                    ActiveSource {
                        version_id: source.clone(),
                        activation: 0.8,
                    },
                ],
                0.75,
            ),
            Err(NetworkError::DuplicateSource(version)) if version == source
        ));
    }
}
