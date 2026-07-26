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

/// Flüchtiges Ergebnis einer einmaligen Aktivierungsweiterleitung.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivatedArtifact {
    pub artifact: Artifact,
    pub activation: f64,
    pub via_weight: f64,
}
