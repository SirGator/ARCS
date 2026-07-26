//! SQLite-Implementierung des unveränderlichen Artefakt-Stores.

use rusqlite::{Connection, OptionalExtension, params};

use crate::core::{Artifact, SchemaRegistry, ValidationError, VersionId, validate_artifact};
use crate::store::network::NetworkEdge;

/// Fehler an der Validierungs- oder Persistenzgrenze.
#[derive(Debug)]
pub enum StoreError {
    /// SQLite konnte eine Operation nicht durchführen.
    Database(rusqlite::Error),
    /// Ein Artefakt konnte nicht verlustfrei serialisiert werden.
    Serialization(serde_json::Error),
    /// Das Artefakt hat seine kontrollierte Prüfung nicht bestanden.
    Validation(ValidationError),
    /// Die neue Version setzt die Historie nicht lückenlos fort.
    VersionConflict { expected: u64, actual: u64 },
    /// Das Kantengewicht liegt außerhalb des geschlossenen Bereichs `-1.0..=1.0`.
    InvalidEdgeWeight(f64),
}

// Diese Konvertierungen halten `?` lesbar und bewahren die Fehlerursache.
impl From<rusqlite::Error> for StoreError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Database(value)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}

/// Append-only SQLite-Speicher für versionierte Artefakte.
///
/// Eine gespeicherte Version wird nie aktualisiert. Neue Erkenntnisse werden
/// als neue Version oder separates, referenzierendes Artefakt angehängt.
pub struct SqliteArtifactStore {
    connection: Connection,
}

impl SqliteArtifactStore {
    /// Öffnet oder erzeugt einen persistenten Store am angegebenen Pfad.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, StoreError> {
        Self::from_connection(Connection::open(path)?)
    }

    /// Erzeugt einen flüchtigen Store für Tests und Demos.
    pub fn in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    /// Initialisiert das idempotent anlegbare Datenbankschema.
    fn from_connection(connection: Connection) -> Result<Self, StoreError> {
        // `version_id` ist global eindeutig. Der zweite UNIQUE-Constraint
        // verhindert konkurrierende Versionen derselben Nummer. `sequence`
        // bewahrt die tatsächliche Commit-Reihenfolge.
        connection.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            CREATE TABLE IF NOT EXISTS artifact_versions (
                sequence       INTEGER PRIMARY KEY AUTOINCREMENT,
                version_id     TEXT NOT NULL UNIQUE,
                artifact_id    TEXT NOT NULL,
                version        INTEGER NOT NULL CHECK (version >= 1),
                stream_key     TEXT NOT NULL,
                schema_id      TEXT NOT NULL,
                artifact_json  TEXT NOT NULL,
                committed_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE (artifact_id, version)
            );
            CREATE INDEX IF NOT EXISTS idx_artifact_versions_stream
                ON artifact_versions(stream_key, sequence);
            CREATE TABLE IF NOT EXISTS artifact_edges (
                sequence         INTEGER PRIMARY KEY AUTOINCREMENT,
                from_version_id  TEXT NOT NULL,
                to_version_id    TEXT NOT NULL,
                weight           REAL NOT NULL CHECK (
                    weight >= -1.0 AND weight <= 1.0
                ),
                connected_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE (from_version_id, to_version_id),
                FOREIGN KEY (from_version_id)
                    REFERENCES artifact_versions(version_id),
                FOREIGN KEY (to_version_id)
                    REFERENCES artifact_versions(version_id)
            );
            CREATE INDEX IF NOT EXISTS idx_artifact_edges_from
                ON artifact_edges(from_version_id, sequence);
            ",
        )?;
        Ok(Self { connection })
    }

    /// Validiert und speichert genau eine unveränderliche Artefaktversion.
    pub fn append(&self, artifact: &Artifact, registry: &SchemaRegistry) -> Result<(), StoreError> {
        // Kein Artefakt darf die Schema-Sicherheitsgrenze umgehen.
        validate_artifact(artifact, registry).map_err(StoreError::Validation)?;

        // Die erste Version muss 1 sein; danach ist nur der direkte Nachfolger
        // erlaubt. So entstehen keine nicht replaybaren Lücken.
        let latest: Option<i64> = self
            .connection
            .query_row(
                "SELECT MAX(version) FROM artifact_versions WHERE artifact_id = ?1",
                params![artifact.artifact_id.0],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        let expected = latest.map_or(1, |version| version as u64 + 1);
        if artifact.version != expected {
            return Err(StoreError::VersionConflict {
                expected,
                actual: artifact.version,
            });
        }

        // Erst nach allen Prüfungen wird das vollständige Artefakt mit einer
        // einzelnen INSERT-Anweisung atomar angehängt.
        let json = serde_json::to_string(artifact)?;
        self.connection.execute(
            "INSERT INTO artifact_versions
             (version_id, artifact_id, version, stream_key, schema_id, artifact_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                artifact.version_id.0,
                artifact.artifact_id.0,
                artifact.version as i64,
                artifact.stream_key,
                artifact.schema_id.0,
                json
            ],
        )?;
        Ok(())
    }

    /// Liest eine exakte Version anhand ihrer globalen Versions-ID.
    pub fn get(&self, version_id: &VersionId) -> Result<Option<Artifact>, StoreError> {
        let json: Option<String> = self
            .connection
            .query_row(
                "SELECT artifact_json FROM artifact_versions WHERE version_id = ?1",
                params![version_id.0],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|json| serde_json::from_str(&json).map_err(StoreError::Serialization))
            .transpose()
    }

    /// Liest die höchste bekannte Version einer Artefaktidentität.
    pub fn latest(&self, artifact_id: &str) -> Result<Option<Artifact>, StoreError> {
        let json: Option<String> = self
            .connection
            .query_row(
                "SELECT artifact_json FROM artifact_versions
                 WHERE artifact_id = ?1 ORDER BY version DESC LIMIT 1",
                params![artifact_id],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|json| serde_json::from_str(&json).map_err(StoreError::Serialization))
            .transpose()
    }

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

    /// Gibt die Anzahl aller gespeicherten Artefaktversionen zurück.
    pub fn len(&self) -> Result<u64, StoreError> {
        let count: i64 =
            self.connection
                .query_row("SELECT COUNT(*) FROM artifact_versions", [], |row| {
                    row.get(0)
                })?;
        Ok(count as u64)
    }

    /// Prüft ohne Mutation, ob der Store noch leer ist.
    pub fn is_empty(&self) -> Result<bool, StoreError> {
        Ok(self.len()? == 0)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::core::{Actor, ActorType, Source, SourceClass, SourceKind, Trust, TrustLevel};

    use super::*;

    fn input() -> Artifact {
        input_with_ids("input-1", "input-1-v1")
    }

    fn input_with_ids(artifact_id: &str, version_id: &str) -> Artifact {
        // Gültiges Input-Artefakt als Ausgangspunkt aller Store-Tests.
        Artifact::new(
            artifact_id,
            version_id,
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
            json!({"raw_text": "Hallo ARCS"}),
        )
    }

    #[test]
    // Der normale Pfad muss exakt denselben Wert wiederherstellen.
    fn appends_and_reads_artifact() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let artifact = input();

        store.append(&artifact, &registry).unwrap();

        assert_eq!(store.len().unwrap(), 1);
        assert_eq!(store.get(&artifact.version_id).unwrap().unwrap(), artifact);
    }

    #[test]
    // Append-only bedeutet insbesondere: keine Version überschreiben.
    fn refuses_to_overwrite_a_version() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let artifact = input();
        store.append(&artifact, &registry).unwrap();

        assert!(store.append(&artifact, &registry).is_err());
        assert_eq!(store.len().unwrap(), 1);
    }

    #[test]
    // Historien mit fehlenden Zwischenversionen sind nicht replaybar.
    fn rejects_version_gaps() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let mut artifact = input();
        artifact.version = 2;
        artifact.version_id = VersionId("input-1-v2".into());

        assert!(matches!(
            store.append(&artifact, &registry),
            Err(StoreError::VersionConflict {
                expected: 1,
                actual: 2
            })
        ));
    }

    #[test]
    // Nur bereits persistierte Artefaktversionen dürfen verbunden werden.
    fn refuses_edges_to_unknown_versions() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let from = input();
        store.append(&from, &registry).unwrap();

        let result = store.connect(&NetworkEdge {
            from: from.version_id,
            to: VersionId("missing-v1".into()),
            weight: 0.75,
        });

        assert!(matches!(result, Err(StoreError::Database(_))));
    }

    #[test]
    // Nur normalisierte Gewichte ergeben eine klar begrenzte Netzsemantik.
    fn refuses_invalid_edge_weights() {
        let store = SqliteArtifactStore::in_memory().unwrap();
        for weight in [f64::NAN, f64::INFINITY, -1.01, 1.01] {
            let result = store.connect(&NetworkEdge {
                from: VersionId("from-v1".into()),
                to: VersionId("to-v1".into()),
                weight,
            });

            assert!(matches!(result, Err(StoreError::InvalidEdgeWeight(value))
                    if value.is_nan() || value == weight));
        }
    }

    #[test]
    // Auch ein direkter Datenbankzugriff darf die Gewichtsgrenze nicht umgehen.
    fn database_constraint_refuses_out_of_range_edge_weights() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let source = input_with_ids("source", "source-v1");
        let target = input_with_ids("target", "target-v1");
        store.append(&source, &registry).unwrap();
        store.append(&target, &registry).unwrap();

        let result = store.connection.execute(
            "INSERT INTO artifact_edges (from_version_id, to_version_id, weight)
             VALUES (?1, ?2, ?3)",
            params![source.version_id.0, target.version_id.0, 1.01],
        );

        assert!(result.is_err());
    }

    #[test]
    // Die gespeicherte Richtung, Reihenfolge und Gewichtung müssen erhalten bleiben.
    fn stores_and_reads_outgoing_edges() {
        let registry = SchemaRegistry::with_bundled_schemas().unwrap();
        let store = SqliteArtifactStore::in_memory().unwrap();
        let source = input_with_ids("source", "source-v1");
        let first = input_with_ids("first", "first-v1");
        let second = input_with_ids("second", "second-v1");
        for artifact in [&source, &first, &second] {
            store.append(artifact, &registry).unwrap();
        }
        store
            .connect(&NetworkEdge {
                from: source.version_id.clone(),
                to: first.version_id.clone(),
                weight: 0.8,
            })
            .unwrap();
        store
            .connect(&NetworkEdge {
                from: source.version_id.clone(),
                to: second.version_id.clone(),
                weight: -0.25,
            })
            .unwrap();

        assert_eq!(
            store.outgoing_edges(&source.version_id).unwrap(),
            vec![
                NetworkEdge {
                    from: source.version_id.clone(),
                    to: first.version_id,
                    weight: 0.8,
                },
                NetworkEdge {
                    from: source.version_id.clone(),
                    to: second.version_id,
                    weight: -0.25,
                },
            ]
        );
    }
}
