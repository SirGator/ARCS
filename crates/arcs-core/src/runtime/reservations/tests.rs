use super::*;
use crate::core::SchemaId;
use crate::core::{Source, SourceClass, SourceKind, Trust, TrustLevel};
use serde_json::json;

struct Time;
impl Clock for Time {
    fn now_rfc3339(&self) -> String {
        "2026-11-17T12:00:00Z".into()
    }
}
fn schemas() -> SchemaRegistry {
    let mut schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    schemas
        .register_json(
            r#"{"$id":"arcs.input.host_runtime.v1","type":"object","additionalProperties":true}"#,
        )
        .unwrap();
    schemas
}
fn seed(store: &SqliteArtifactStore, schemas: &SchemaRegistry) -> ReservationOwner {
    for (id, schema) in [
        ("definition", "arcs.input.host_runtime.v1"),
        ("input", "arcs.input.v1"),
    ] {
        let artifact = Artifact::new(
            id,
            format!("{id}-v1"),
            "input",
            schema,
            "2026-11-17T12:00:00Z",
            Actor {
                actor_type: ActorType::System,
                id: "arcs.host.runtime".into(),
            },
            Source {
                kind: SourceKind::Internal,
                reference: id.into(),
            },
            Trust {
                level: TrustLevel::High,
                source_class: SourceClass::System,
            },
            id,
            if id == "input" {
                json!({"raw_text":"test"})
            } else {
                json!({})
            },
        );
        store.append(&artifact, schemas).unwrap();
    }
    ReservationOwner {
        definition_version: VersionId("definition-v1".into()),
        input_version: VersionId("input-v1".into()),
        task_sequence: 1,
        session: 1,
    }
}
fn actor() -> Actor {
    Actor {
        actor_type: ActorType::Human,
        id: "operator".into(),
    }
}

#[test]
fn all_or_nothing_ownership_is_versioned_and_stale_releases_cannot_touch_a_new_owner() {
    let store = SqliteArtifactStore::in_memory().unwrap();
    let schemas = schemas();
    let owner = seed(&store, &schemas);
    let service = ReservationService::new(&store, &schemas, &Time);
    let held = service.acquire(&owner, &["b".into()]).unwrap();
    let before = store.len().unwrap();
    assert_eq!(
        service.acquire(&owner, &["b".into()]).unwrap()[0].version_id,
        held[0].version_id
    );
    assert_eq!(store.len().unwrap(), before);
    let mut other = owner.clone();
    other.task_sequence = 2;
    assert!(matches!(
        service.acquire(&other, &["a".into(), "b".into()]),
        Err(ReservationError::Busy { .. })
    ));
    assert!(service.current("a").unwrap().is_none());
    assert!(
        service
            .release(
                &other,
                std::slice::from_ref(&held[0].version_id),
                &actor(),
                "wrong owner",
                &[]
            )
            .is_err()
    );
    service
        .release(
            &owner,
            std::slice::from_ref(&held[0].version_id),
            &actor(),
            "checked",
            std::slice::from_ref(&owner.input_version),
        )
        .unwrap();
    let released_count = store.len().unwrap();
    service
        .release(
            &owner,
            std::slice::from_ref(&held[0].version_id),
            &actor(),
            "checked",
            std::slice::from_ref(&owner.input_version),
        )
        .unwrap();
    assert_eq!(store.len().unwrap(), released_count);
    assert!(service.acquire(&owner, &["b".into()]).is_err());
    let newer = service.acquire(&other, &["b".into()]).unwrap();
    assert_ne!(newer[0].version_id, held[0].version_id);
    assert!(
        service
            .release(
                &owner,
                std::slice::from_ref(&held[0].version_id),
                &actor(),
                "stale",
                &[]
            )
            .is_err()
    );
    assert_eq!(service.current("b").unwrap().unwrap().owner, other);
}

#[test]
fn invalid_request_or_model_release_never_changes_a_reservation() {
    let store = SqliteArtifactStore::in_memory().unwrap();
    let schemas = schemas();
    let owner = seed(&store, &schemas);
    let service = ReservationService::new(&store, &schemas, &Time);
    for resources in [
        vec![],
        vec!["duplicate".into(), "duplicate".into()],
        vec!["bad/name".into()],
    ] {
        assert!(service.acquire(&owner, &resources).is_err());
    }
    let held = service.acquire(&owner, &["device".into()]).unwrap();
    let before = store.len().unwrap();
    assert!(
        service
            .release(
                &owner,
                std::slice::from_ref(&held[0].version_id),
                &Actor {
                    actor_type: ActorType::Model,
                    id: "model".into()
                },
                "not authorized",
                &[]
            )
            .is_err()
    );
    assert_eq!(store.len().unwrap(), before);
}

#[test]
fn failed_batch_commit_does_not_leave_partial_ownership() {
    let store = SqliteArtifactStore::in_memory().unwrap();
    let schemas = schemas();
    let owner = seed(&store, &schemas);
    // A missing provenance parent fails inside the same write transaction.
    let service = ReservationService::new(&store, &schemas, &Time);
    let first = records::make(
        "a",
        &owner,
        ReservationState::Held,
        None,
        &actor(),
        "test",
        &[],
        &Time,
        &schemas,
    )
    .unwrap();
    let second = records::make(
        "b",
        &owner,
        ReservationState::Held,
        None,
        &actor(),
        "test",
        &[VersionId("missing-evidence".into())],
        &Time,
        &schemas,
    )
    .unwrap();
    assert!(
        store
            .append_current_compared(
                &[first, second],
                &schemas,
                &[
                    (records::subject("a"), SchemaId(SCHEMA.into()), None),
                    (records::subject("b"), SchemaId(SCHEMA.into()), None)
                ]
            )
            .is_err()
    );
    assert!(service.current("a").unwrap().is_none());
    assert!(service.current("b").unwrap().is_none());
    assert_eq!(store.len().unwrap(), 2);
    let held = service.acquire(&owner, &["x".into()]).unwrap();
    let next = records::make(
        "y",
        &owner,
        ReservationState::Held,
        None,
        &actor(),
        "test",
        &[],
        &Time,
        &schemas,
    )
    .unwrap();
    assert!(
        store
            .append_current_compared(
                &[next],
                &schemas,
                &[
                    (
                        records::subject("x"),
                        SchemaId(SCHEMA.into()),
                        Some(VersionId("stale-revision".into()))
                    ),
                    (records::subject("y"), SchemaId(SCHEMA.into()), None)
                ]
            )
            .is_err()
    );
    assert!(service.current("y").unwrap().is_none());
    assert_eq!(
        service.current("x").unwrap().unwrap().version_id,
        held[0].version_id
    );
}

#[test]
fn independent_connections_contend_atomically_and_keep_ownership_after_reopen() {
    use std::sync::{Arc, Barrier};
    let path = std::env::temp_dir().join(format!(
        "arcs-exclusive-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let schemas = schemas();
    let store = SqliteArtifactStore::open(&path).unwrap();
    let owner = seed(&store, &schemas);
    drop(store);
    let left = SqliteArtifactStore::open(&path).unwrap();
    let right = SqliteArtifactStore::open(&path).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let threads = [left, right]
        .into_iter()
        .enumerate()
        .map(|(index, store)| {
            let mut owner = owner.clone();
            owner.task_sequence += index as u64;
            let schemas = schemas.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                ReservationService::new(&store, &schemas, &Time).acquire(&owner, &["device".into()])
            })
        })
        .collect::<Vec<_>>();
    let outcomes = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes.iter().filter(|r| r.is_ok()).count(),
        1,
        "{outcomes:?}"
    );
    let reopened = SqliteArtifactStore::open(&path).unwrap();
    assert_eq!(
        ReservationService::new(&reopened, &schemas, &Time)
            .current("device")
            .unwrap()
            .unwrap()
            .state,
        ReservationState::Held
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}
