use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::core::{Actor, ActorType, Source, SourceClass, SourceKind, Trust, TrustLevel};

use super::*;

struct TemporaryDatabase {
    path: PathBuf,
}

impl TemporaryDatabase {
    fn new() -> Self {
        static NEXT_DATABASE: AtomicU64 = AtomicU64::new(1);
        let sequence = NEXT_DATABASE.fetch_add(1, Ordering::Relaxed);
        Self {
            path: std::env::temp_dir().join(format!(
                "arcs-artifact-history-{}-{sequence}.sqlite",
                std::process::id()
            )),
        }
    }
}

impl Drop for TemporaryDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

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

#[test]
fn current_state_replaces_pointer_but_preserves_history() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let mut first = input_with_ids("cpu-state", "cpu-state-v1").with_subject("server-01/cpu");
    first.payload = json!({"raw_text": "cpu: 0.50"});
    let mut second = first.clone();
    second.version = 2;
    second.version_id = VersionId("cpu-state-v2".into());
    second.payload = json!({"raw_text": "cpu: 0.92"});

    store.append_current(&first, &registry).unwrap();
    store.append_current(&second, &registry).unwrap();

    assert_eq!(
        store
            .current(
                &SubjectId("server-01/cpu".into()),
                &SchemaId("arcs.input.v1".into()),
            )
            .unwrap(),
        Some(second)
    );
    assert_eq!(store.get(&first.version_id).unwrap(), Some(first));
    assert_eq!(store.len().unwrap(), 2);
}

#[test]
fn subject_history_survives_reopening_the_store_in_commit_order() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let database = TemporaryDatabase::new();
    let subject = SubjectId("server-01/cpu".into());
    let schema_id = SchemaId("arcs.input.v1".into());
    let mut first =
        input_with_ids("cpu-observation-1", "cpu-observation-1-v1").with_subject(subject.0.clone());
    first.payload = json!({"raw_text": "cpu: 0.50"});
    let mut second =
        input_with_ids("cpu-observation-2", "cpu-observation-2-v1").with_subject(subject.0.clone());
    second.payload = json!({"raw_text": "cpu: 0.92"});

    {
        let store = SqliteArtifactStore::open(&database.path).unwrap();
        store.append_current(&first, &registry).unwrap();
        store.append_current(&second, &registry).unwrap();
    }

    let reopened = SqliteArtifactStore::open(&database.path).unwrap();
    assert_eq!(
        reopened.history(&subject, &schema_id).unwrap(),
        vec![first, second.clone()]
    );
    assert_eq!(
        reopened.current(&subject, &schema_id).unwrap(),
        Some(second)
    );
}
