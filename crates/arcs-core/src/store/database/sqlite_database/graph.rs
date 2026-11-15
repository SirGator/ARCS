//! SQL für gewichtete Kanten und typisierte Relationen.
use super::*;
use crate::store::{ArtifactRelation, NetworkEdge};

impl SqliteArtifactStore {
    /// Verbindet zwei bereits gespeicherte Artefaktversionen.
    ///
    /// Die Fremdschlüssel verhindern Kanten zu unbekannten Versionen. Eine
    /// gerichtete Verbindung darf nur einmal angelegt werden.
    pub(crate) fn connect(&self, edge: &NetworkEdge) -> Result<(), StoreError> {
        if !edge.weight.is_finite() || !(-1.0..=1.0).contains(&edge.weight) {
            return Err(StoreError::InvalidEdgeWeight(edge.weight));
        }

        self.connection.execute(
            "INSERT INTO artifact_edges (from_version_id, to_version_id, weight)
             VALUES (?1, ?2, ?3)",
            params![edge.from.0, edge.to.0, edge.weight],
        )?;
        Ok(())
    }

    /// Liest ausgehende Kanten in stabiler Einfügereihenfolge.
    pub(crate) fn outgoing_edges(&self, from: &VersionId) -> Result<Vec<NetworkEdge>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT from_version_id, to_version_id, weight
             FROM artifact_edges
             WHERE from_version_id = ?1
             ORDER BY sequence",
        )?;
        let edges = statement
            .query_map(params![from.0], |row| {
                Ok(NetworkEdge {
                    from: VersionId(row.get(0)?),
                    to: VersionId(row.get(1)?),
                    weight: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(edges)
    }

    /// Liest genau eine gerichtete Kante zwischen zwei Artifact-Versionen.
    pub(crate) fn edge(
        &self,
        from: &VersionId,
        to: &VersionId,
    ) -> Result<Option<NetworkEdge>, StoreError> {
        self.connection
            .query_row(
                "SELECT from_version_id, to_version_id, weight
                 FROM artifact_edges
                 WHERE from_version_id = ?1 AND to_version_id = ?2",
                params![from.0, to.0],
                |row| {
                    Ok(NetworkEdge {
                        from: VersionId(row.get(0)?),
                        to: VersionId(row.get(1)?),
                        weight: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(StoreError::from)
    }

    /// Aktualisiert ausschließlich das Gewicht einer bereits bekannten Kante.
    pub(crate) fn update_edge_weight(
        &self,
        from: &VersionId,
        to: &VersionId,
        weight: f64,
    ) -> Result<(), StoreError> {
        if !weight.is_finite() || !(-1.0..=1.0).contains(&weight) {
            return Err(StoreError::InvalidEdgeWeight(weight));
        }

        self.connection.execute(
            "UPDATE artifact_edges SET weight = ?3
             WHERE from_version_id = ?1 AND to_version_id = ?2",
            params![from.0, to.0, weight],
        )?;
        Ok(())
    }

    /// Speichert eine semantische Relation unabhängig von Netzgewichten.
    pub(crate) fn connect_relation(&self, relation: &ArtifactRelation) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO artifact_relations
             (from_version_id, to_version_id, relation_kind)
             VALUES (?1, ?2, ?3)",
            params![relation.from.0, relation.to.0, relation.kind.as_str()],
        )?;
        Ok(())
    }

    /// Liest semantische Relationen in stabiler Einfügereihenfolge.
    pub(crate) fn outgoing_relations(
        &self,
        from: &VersionId,
    ) -> Result<Vec<ArtifactRelation>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT from_version_id, to_version_id, relation_kind
             FROM artifact_relations
             WHERE from_version_id = ?1
             ORDER BY sequence",
        )?;
        let rows = statement
            .query_map(params![from.0], |row| {
                let raw_kind: String = row.get(2)?;
                let kind = RelationKind::new(raw_kind).map_err(|_| {
                    rusqlite::Error::InvalidColumnType(
                        2,
                        "relation_kind".into(),
                        rusqlite::types::Type::Text,
                    )
                })?;
                Ok(ArtifactRelation {
                    from: VersionId(row.get(0)?),
                    to: VersionId(row.get(1)?),
                    kind,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}
