//! Nachweise der Schutzstufen-Auflösung im Store.
//!
//! Die Tests decken die drei entscheidenden Fälle ab: Vererbung aus der
//! Herkunft, Ablehnung einer nicht belegten Herabstufung und die Wiederaufnahme
//! nach einem abgelehnten Commit.

use serde_json::json;

use crate::core::security::SecurityProfile;
use crate::core::{
    Actor, ActorType, Artifact, Classification, Provenance, Source, SourceClass, SourceKind, Trust,
    TrustLevel, VersionId,
};

use super::*;

/// Ein Eingangsartefakt mit frei wählbarer Herkunft und Schutzstufe.
fn artifact(
    artifact_id: &str,
    version_id: &str,
    version: u64,
    actor_type: ActorType,
    parents: Vec<&str>,
    security: SecurityProfile,
) -> Artifact {
    let mut artifact = Artifact::new(
        artifact_id,
        version_id,
        "input",
        "arcs.input.v1",
        "2026-11-28T00:00:00Z",
        Actor {
            actor_type,
            id: "producer".into(),
        },
        Source {
            kind: SourceKind::Internal,
            reference: "test".into(),
        },
        Trust {
            level: TrustLevel::Medium,
            source_class: SourceClass::System,
        },
        format!("stream:{artifact_id}"),
        json!({"raw_text": "Inhalt"}),
    );
    artifact.version = version;
    if !parents.is_empty() {
        artifact.provenance = Some(Provenance {
            parents: parents.into_iter().map(str::to_owned).collect(),
            ..Default::default()
        });
    }
    artifact.security = security;
    artifact
}

fn confidential(artifact_id: &str, version_id: &str) -> Artifact {
    artifact(
        artifact_id,
        version_id,
        1,
        ActorType::System,
        vec![],
        SecurityProfile::declared(Classification::Confidential),
    )
}

#[test]
fn stores_the_declared_classification() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let artifact = confidential("note-1", "note-1-v1");

    let stored = store.append(&artifact, &registry).unwrap();

    assert_eq!(stored.security.effective(), Classification::Confidential);
    assert_eq!(
        store
            .get(&artifact.version_id)
            .unwrap()
            .unwrap()
            .security
            .effective(),
        Classification::Confidential
    );
}

#[test]
fn inherits_the_classification_of_its_sources() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    store
        .append(&confidential("note-1", "note-1-v1"), &registry)
        .unwrap();

    // Keine Deklaration: Die abgeleitete Notiz erbt die Stufe der Quelle.
    let derived = artifact(
        "note-2",
        "note-2-v1",
        1,
        ActorType::System,
        vec!["note-1-v1"],
        SecurityProfile::default(),
    );
    let stored = store.append(&derived, &registry).unwrap();

    assert_eq!(stored.security.effective(), Classification::Confidential);
}

#[test]
fn rejects_an_unauthorized_downgrade() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    store
        .append(&confidential("note-1", "note-1-v1"), &registry)
        .unwrap();

    let downgrade = artifact(
        "note-2",
        "note-2-v1",
        1,
        ActorType::System,
        vec!["note-1-v1"],
        SecurityProfile::declared(Classification::Public),
    );
    let error = store.append(&downgrade, &registry).unwrap_err();

    assert!(matches!(
        error,
        StoreError::UnauthorizedDowngrade {
            declared: Classification::Public,
            inherited: Classification::Confidential,
        }
    ));
    // Der abgelehnte Versuch hinterlässt keine Spur im Store.
    assert_eq!(store.len().unwrap(), 1);
    assert!(store.get(&VersionId("note-2-v1".into())).unwrap().is_none());
}

#[test]
fn accepts_a_documented_declassification() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    store
        .append(&confidential("note-1", "note-1-v1"), &registry)
        .unwrap();

    let redacted = artifact(
        "note-2",
        "note-2-v1",
        1,
        ActorType::System,
        vec!["note-1-v1"],
        SecurityProfile::declassified(
            VersionId("note-1-v1".into()),
            Classification::Internal,
            "operator approved release",
        ),
    );
    let stored = store.append(&redacted, &registry).unwrap();

    assert_eq!(stored.security.effective(), Classification::Internal);
    assert_eq!(
        stored.security.declassified_from,
        Some(VersionId("note-1-v1".into()))
    );
}

#[test]
fn rejects_a_declassification_by_a_model() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    store
        .append(&confidential("note-1", "note-1-v1"), &registry)
        .unwrap();

    let redacted = artifact(
        "note-2",
        "note-2-v1",
        1,
        ActorType::Model,
        vec!["note-1-v1"],
        SecurityProfile::declassified(
            VersionId("note-1-v1".into()),
            Classification::Internal,
            "model claims approval",
        ),
    );
    let error = store.append(&redacted, &registry).unwrap_err();

    assert!(matches!(error, StoreError::IncompleteDeclassification));
    assert!(store.get(&VersionId("note-2-v1".into())).unwrap().is_none());
}

#[test]
fn a_new_version_cannot_be_less_sensitive_than_its_predecessor() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    store
        .append(&confidential("note-1", "note-1-v1"), &registry)
        .unwrap();

    let revision = artifact(
        "note-1",
        "note-1-v2",
        2,
        ActorType::System,
        vec![],
        SecurityProfile::declared(Classification::Internal),
    );
    let error = store.append(&revision, &registry).unwrap_err();

    assert!(matches!(
        error,
        StoreError::UnauthorizedDowngrade {
            declared: Classification::Internal,
            inherited: Classification::Confidential,
        }
    ));
    // Die erste Version bleibt unberührt und der Store bleibt benutzbar.
    assert_eq!(store.len().unwrap(), 1);
    let recovered = store.append(
        &artifact(
            "note-1",
            "note-1-v2",
            2,
            ActorType::System,
            vec![],
            SecurityProfile::declared(Classification::Confidential),
        ),
        &registry,
    );
    assert!(recovered.is_ok());
}

#[test]
fn a_missing_source_does_not_lower_the_classification() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    store
        .append(&confidential("note-1", "note-1-v1"), &registry)
        .unwrap();

    // Die Quelle existiert nicht. Es wird nichts geerbt, aber auch nichts
    // herabgestuft: Die Deklaration gilt, weil keine strengere Quelle
    // nachweisbar ist.
    let orphan = artifact(
        "note-3",
        "note-3-v1",
        1,
        ActorType::System,
        vec!["missing-v1"],
        SecurityProfile::declared(Classification::Public),
    );
    let stored = store.append(&orphan, &registry).unwrap();
    assert_eq!(stored.security.effective(), Classification::Public);
}

#[test]
fn legacy_artifacts_without_a_security_profile_stay_readable() {
    let registry = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let legacy = artifact(
        "note-1",
        "note-1-v1",
        1,
        ActorType::System,
        vec![],
        SecurityProfile::default(),
    );
    store.append(&legacy, &registry).unwrap();

    let stored = store.get(&VersionId("note-1-v1".into())).unwrap().unwrap();
    assert_eq!(stored.security.effective(), Classification::Internal);
    // Ohne Abweichung bleibt das gespeicherte Artefakt unverändert.
    assert_eq!(stored, legacy);
}
