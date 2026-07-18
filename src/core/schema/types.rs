use crate::core::artifact::{ArtifactKind, SchemaId};

#[derive(Debug, Clone)]
pub struct SchemaDefinition {
    pub id: SchemaId,
    pub name: String,
    pub artifact_kind: ArtifactKind,
}