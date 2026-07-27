//! Vertikaler Slice für semantische Artifact-Beziehungen.

use crate::core::VersionId;
use crate::store::{ArtifactRelation, RelationKind, SqliteArtifactStore, StoreError};

/// Fachliche Grenze für auditierbare Relationen ohne Aktivierungssemantik.
pub struct ArtifactRelations<'a> {
    store: &'a SqliteArtifactStore,
}

impl<'a> ArtifactRelations<'a> {
    pub fn new(store: &'a SqliteArtifactStore) -> Self {
        Self { store }
    }

    pub fn connect(
        &self,
        from: VersionId,
        to: VersionId,
        kind: RelationKind,
    ) -> Result<(), StoreError> {
        self.store
            .connect_relation(&ArtifactRelation { from, to, kind })
    }

    pub fn outgoing(&self, from: &VersionId) -> Result<Vec<ArtifactRelation>, StoreError> {
        self.store.outgoing_relations(from)
    }
}

#[cfg(test)]
mod tests;
