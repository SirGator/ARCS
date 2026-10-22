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
    /// Eine Aktivierung darf nicht auf eine unbekannte Artifact-Version zeigen.
    MissingSource(VersionId),
    /// Ein Gewichtsupdate darf keine neue strukturelle Verbindung erzeugen.
    MissingEdge { from: VersionId, to: VersionId },
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

    /// Liest genau eine gerichtete Kante, falls sie bereits existiert.
    pub fn edge(
        &self,
        from: &VersionId,
        to: &VersionId,
    ) -> Result<Option<NetworkEdge>, NetworkError> {
        Ok(self.store.edge(from, to)?)
    }

    /// Ändert das Gewicht einer bestehenden Kante, ohne Struktur zu erzeugen.
    pub fn set_weight(
        &self,
        from: &VersionId,
        to: &VersionId,
        weight: f64,
    ) -> Result<(), NetworkError> {
        if self.edge(from, to)?.is_none() {
            return Err(NetworkError::MissingEdge {
                from: from.clone(),
                to: to.clone(),
            });
        }
        self.store.update_edge_weight(from, to, weight)?;
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
            if self.store.get(&source.version_id)?.is_none() {
                return Err(NetworkError::MissingSource(source.version_id.clone()));
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
mod tests;
