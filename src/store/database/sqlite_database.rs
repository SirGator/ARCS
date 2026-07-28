//! SQLite-Implementierung des unveränderlichen Artefakt-Stores.

use rusqlite::{Connection, OptionalExtension, params};

use crate::core::{
    Artifact, SchemaId, SchemaRegistry, SubjectId, ValidationError, VersionId, validate_artifact,
};
use crate::store::{ArtifactRelation, NetworkEdge, RelationKind};

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
    /// Nur ein Artifact mit Subject kann einen Current-State-Zeiger bilden.
    MissingSubject,
    /// Eine bereits persistierte Schema-ID darf nie einen anderen Vertrag erhalten.
    SchemaDrift(SchemaId),
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
                subject        TEXT,
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
            CREATE TABLE IF NOT EXISTS artifact_relations (
                sequence         INTEGER PRIMARY KEY AUTOINCREMENT,
                from_version_id  TEXT NOT NULL,
                to_version_id    TEXT NOT NULL,
                relation_kind    TEXT NOT NULL,
                related_at       TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                UNIQUE (from_version_id, to_version_id, relation_kind),
                FOREIGN KEY (from_version_id)
                    REFERENCES artifact_versions(version_id),
                FOREIGN KEY (to_version_id)
                    REFERENCES artifact_versions(version_id)
            );
            CREATE INDEX IF NOT EXISTS idx_artifact_relations_from
                ON artifact_relations(from_version_id, sequence);
            CREATE TABLE IF NOT EXISTS current_artifacts (
                subject       TEXT NOT NULL,
                schema_id     TEXT NOT NULL,
                version_id    TEXT NOT NULL UNIQUE,
                updated_at    TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (subject, schema_id),
                FOREIGN KEY (version_id)
                    REFERENCES artifact_versions(version_id)
            );
            CREATE TABLE IF NOT EXISTS schema_bindings (
                schema_id    TEXT PRIMARY KEY,
                schema_json  TEXT NOT NULL
            );
            ",
        )?;
        ensure_subject_history_schema(&connection)?;
        Ok(Self { connection })
    }

    /// Validiert und speichert genau eine unveränderliche Artefaktversion.
    pub(crate) fn append(
        &self,
        artifact: &Artifact,
        registry: &SchemaRegistry,
    ) -> Result<(), StoreError> {
        // Kein Artefakt darf die Schema-Sicherheitsgrenze umgehen.
        validate_artifact(artifact, registry).map_err(StoreError::Validation)?;
        append_validated(&self.connection, artifact)
    }

    /// Bindet alle bekannten Schema-IDs atomar an ihre kanonischen Dokumente.
    ///
    /// Eine schon persistierte ID darf nur mit demselben Dokument erneut
    /// gebunden werden. Schema-Evolution benötigt deshalb immer eine neue ID.
    pub(crate) fn bind_schemas(&self, registry: &SchemaRegistry) -> Result<(), StoreError> {
        let transaction = self.connection.unchecked_transaction()?;
        for schema in registry.definitions() {
            let canonical = serde_json::to_string(&schema.document)?;
            let existing: Option<String> = transaction
                .query_row(
                    "SELECT schema_json FROM schema_bindings WHERE schema_id = ?1",
                    params![schema.id.0],
                    |row| row.get(0),
                )
                .optional()?;
            match existing {
                Some(existing) if existing != canonical => {
                    return Err(StoreError::SchemaDrift(schema.id.clone()));
                }
                Some(_) => {}
                None => {
                    transaction.execute(
                        "INSERT INTO schema_bindings (schema_id, schema_json) VALUES (?1, ?2)",
                        params![schema.id.0, canonical],
                    )?;
                }
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Hängt eine historische Version an und setzt im selben Commit den
    /// aktuellen Zustand ihres `(subject, schema_id)`-Slots.
    pub(crate) fn append_current(
        &self,
        artifact: &Artifact,
        registry: &SchemaRegistry,
    ) -> Result<(), StoreError> {
        self.append_current_related(artifact, registry, &[])
    }

    /// Atomare Current-State-Aufnahme mit ausgehenden semantischen Relationen.
    pub(crate) fn append_current_related(
        &self,
        artifact: &Artifact,
        registry: &SchemaRegistry,
        relations: &[(VersionId, RelationKind)],
    ) -> Result<(), StoreError> {
        validate_artifact(artifact, registry).map_err(StoreError::Validation)?;
        let subject = artifact
            .subject
            .as_ref()
            .ok_or(StoreError::MissingSubject)?;

        let transaction = self.connection.unchecked_transaction()?;
        append_validated(&transaction, artifact)?;
        transaction.execute(
            "INSERT INTO current_artifacts (subject, schema_id, version_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(subject, schema_id) DO UPDATE SET
                 version_id = excluded.version_id,
                 updated_at = CURRENT_TIMESTAMP",
            params![subject.0, artifact.schema_id.0, artifact.version_id.0],
        )?;
        insert_relations(&transaction, &artifact.version_id, relations)?;
        transaction.commit()?;
        Ok(())
    }

    /// Atomare Event-Aufnahme mit ausgehenden semantischen Relationen.
    pub(crate) fn append_related(
        &self,
        artifact: &Artifact,
        registry: &SchemaRegistry,
        relations: &[(VersionId, RelationKind)],
    ) -> Result<(), StoreError> {
        validate_artifact(artifact, registry).map_err(StoreError::Validation)?;
        let transaction = self.connection.unchecked_transaction()?;
        append_validated(&transaction, artifact)?;
        insert_relations(&transaction, &artifact.version_id, relations)?;
        transaction.commit()?;
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

    /// Liest ausschließlich die aktuellste Sicht auf ein fachliches Subject.
    ///
    /// Historische Versionen bleiben weiterhin über `get` erreichbar.
    pub fn current(
        &self,
        subject: &SubjectId,
        schema_id: &SchemaId,
    ) -> Result<Option<Artifact>, StoreError> {
        let json: Option<String> = self
            .connection
            .query_row(
                "SELECT versions.artifact_json
                 FROM current_artifacts AS current
                 JOIN artifact_versions AS versions
                   ON versions.version_id = current.version_id
                 WHERE current.subject = ?1 AND current.schema_id = ?2",
                params![subject.0, schema_id.0],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|json| serde_json::from_str(&json).map_err(StoreError::Serialization))
            .transpose()
    }

    /// Liest alle gespeicherten Sichtstände eines `(subject, schema_id)`-Slots.
    ///
    /// Die Reihenfolge entspricht der unveränderlichen Commit-Reihenfolge und
    /// bleibt deshalb auch nach einem Neustart deterministisch replaybar.
    pub fn history(
        &self,
        subject: &SubjectId,
        schema_id: &SchemaId,
    ) -> Result<Vec<Artifact>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT artifact_json
             FROM artifact_versions
             WHERE subject = ?1 AND schema_id = ?2
             ORDER BY sequence",
        )?;
        let rows = statement.query_map(params![subject.0, schema_id.0], |row| {
            row.get::<_, String>(0)
        })?;

        rows.map(|row| {
            let json = row?;
            serde_json::from_str(&json).map_err(StoreError::Serialization)
        })
        .collect()
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

/// Führt den gemeinsamen append-only Teil auf einer Connection oder
/// Transaction aus. Validierung erfolgt immer vor dem Aufruf.
fn append_validated(connection: &Connection, artifact: &Artifact) -> Result<(), StoreError> {
    // Die erste Version muss 1 sein; danach ist nur der direkte Nachfolger
    // erlaubt. So entstehen keine nicht replaybaren Lücken.
    let latest: Option<i64> = connection
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

    let json = serde_json::to_string(artifact)?;
    connection.execute(
        "INSERT INTO artifact_versions
         (version_id, artifact_id, version, stream_key, schema_id, subject, artifact_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            artifact.version_id.0,
            artifact.artifact_id.0,
            artifact.version as i64,
            artifact.stream_key,
            artifact.schema_id.0,
            artifact.subject.as_ref().map(|subject| &subject.0),
            json
        ],
    )?;
    Ok(())
}

/// Ergänzt ältere Datenbanken um die indexierbare Subject-Spalte.
///
/// Frühe ARCS-Stores hielten das Subject nur im JSON-Umschlag. Beim ersten
/// Öffnen wird es einmalig übernommen; danach pflegt jeder Append die Spalte
/// direkt. Dadurch bleiben bestehende Historien vollständig abfragbar.
fn ensure_subject_history_schema(connection: &Connection) -> Result<(), StoreError> {
    let has_subject: bool = connection.query_row(
        "SELECT EXISTS (
             SELECT 1
             FROM pragma_table_info('artifact_versions')
             WHERE name = 'subject'
         )",
        [],
        |row| row.get(0),
    )?;

    if has_subject {
        connection.execute(
            "CREATE INDEX IF NOT EXISTS idx_artifact_versions_subject_schema
             ON artifact_versions(subject, schema_id, sequence)
             WHERE subject IS NOT NULL",
            [],
        )?;
        return Ok(());
    }

    // SQLite führt auch Schemaänderungen transaktional aus. Ein Abbruch kann
    // daher keine halb migrierte Datenbank mit leerer Subject-Spalte lassen.
    let transaction = connection.unchecked_transaction()?;
    transaction.execute("ALTER TABLE artifact_versions ADD COLUMN subject TEXT", [])?;

    let legacy_rows = {
        let mut statement =
            transaction.prepare("SELECT version_id, artifact_json FROM artifact_versions")?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };

    for (version_id, json) in legacy_rows {
        let artifact: Artifact = serde_json::from_str(&json)?;
        if let Some(subject) = artifact.subject {
            transaction.execute(
                "UPDATE artifact_versions SET subject = ?1 WHERE version_id = ?2",
                params![subject.0, version_id],
            )?;
        }
    }

    transaction.execute(
        "CREATE INDEX IF NOT EXISTS idx_artifact_versions_subject_schema
         ON artifact_versions(subject, schema_id, sequence)
         WHERE subject IS NOT NULL",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

fn insert_relations(
    connection: &Connection,
    from: &VersionId,
    relations: &[(VersionId, RelationKind)],
) -> Result<(), StoreError> {
    for (to, kind) in relations {
        connection.execute(
            "INSERT INTO artifact_relations
             (from_version_id, to_version_id, relation_kind)
             VALUES (?1, ?2, ?3)",
            params![from.0, to.0, kind.as_str()],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
