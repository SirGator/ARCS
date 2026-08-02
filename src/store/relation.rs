use serde::{Deserialize, Serialize};

use crate::core::VersionId;

/// Semantischer, gerichteter Beziehungstyp zwischen zwei Artifact-Versionen.
///
/// Anders als `NetworkEdge` beeinflusst eine Relation keine Aktivierung. Sie
/// dokumentiert nachvollziehbar, warum Artefakte zusammengehören.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RelationKind(String);

impl RelationKind {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidRelationKind> {
        let value = value.into();
        if value.is_empty()
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.')
            })
        {
            return Err(InvalidRelationKind(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Ein ungültiger Relationsname würde nicht stabil serialisierbar sein.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidRelationKind(pub String);

/// Persistierte, gerichtete und typisierte Artifact-Beziehung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRelation {
    pub from: VersionId,
    pub to: VersionId,
    pub kind: RelationKind,
}

/// Häufige, aber nicht abschließende Relationen des Agentenflows.
pub mod relation_kinds {
    use super::RelationKind;

    fn known(value: &str) -> RelationKind {
        // Alle Konstanten dieses Moduls erfüllen den validierten Vertrag.
        RelationKind::new(value).expect("built-in relation kind must be valid")
    }

    pub fn fulfills() -> RelationKind {
        known("fulfills")
    }

    pub fn caused_by() -> RelationKind {
        known("caused_by")
    }

    pub fn derived_from() -> RelationKind {
        known("derived_from")
    }

    pub fn supported_by() -> RelationKind {
        known("supported_by")
    }

    pub fn generated_by() -> RelationKind {
        known("generated_by")
    }

    pub fn result_of() -> RelationKind {
        known("result_of")
    }
}
