use serde::{Deserialize, Serialize};

use crate::core::{Artifact, VersionId};

/// Persistierte, gerichtete Beziehung zwischen zwei Artefaktversionen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkEdge {
    pub from: VersionId,
    pub to: VersionId,
    pub weight: f64,
}

/// Aufgelöster Nachbar einschließlich der Gewichtung seiner Kante.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkNeighbor {
    pub artifact: Artifact,
    pub weight: f64,
}

/// Flüchtige Aktivierung einer gespeicherten Quellversion.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveSource {
    pub version_id: VersionId,
    pub activation: f64,
}

/// Flüchtiges Ziel, dessen aggregierte Aktivierung die Schwelle erreicht.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivatedArtifact {
    pub artifact: Artifact,
    pub activation: f64,
}
