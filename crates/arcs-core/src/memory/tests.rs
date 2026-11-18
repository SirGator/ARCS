use super::*;
use serde_json::json;

const NOW: &str = "2026-11-18T12:00:00Z";
struct FixedClock;
impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        NOW.into()
    }
}
fn source(store: &SqliteArtifactStore, schemas: &SchemaRegistry, id: &str) -> Artifact {
    let artifact = Artifact::new(
        id,
        format!("{id}-v1"),
        "input",
        "arcs.input.v1",
        NOW,
        Actor {
            actor_type: ActorType::Model,
            id: "untrusted-model".into(),
        },
        Source {
            kind: SourceKind::External,
            reference: id.into(),
        },
        Trust {
            level: TrustLevel::Low,
            source_class: SourceClass::Model,
        },
        id,
        json!({"raw_text":"a proposed claim"}),
    );
    store.append(&artifact, schemas).unwrap();
    artifact
}
fn command(id: &VersionId) -> MemoryAssessmentCommand {
    MemoryAssessmentCommand {
        target_version_id: id.clone(),
        expected_version_id: None,
        decision_by: Actor {
            actor_type: ActorType::Human,
            id: "operator".into(),
        },
        reason: "reviewed evidence".into(),
        status: MemoryStatus::Active,
        valid_from: None,
        valid_until: None,
        evidence: vec![],
        replacement_version_id: None,
    }
}
fn strict() -> MemoryEpistemicPolicy {
    MemoryEpistemicPolicy {
        allow_unassessed: false,
        allow_disputed: false,
    }
}

#[test]
fn revisions_preserve_source_trust_evidence_and_exact_latest_replay() {
    let store = SqliteArtifactStore::in_memory().unwrap();
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let old = source(&store, &schemas, "old");
    let replacement = source(&store, &schemas, "new");
    let service = MemoryAssessmentService {
        store: &store,
        schemas: &schemas,
        clock: &FixedClock,
    };
    let mut c = command(&old.version_id);
    c.evidence.push(replacement.version_id.clone());
    let first = service.assess(&c).unwrap();
    let count = store.len().unwrap();
    assert_eq!(service.assess(&c).unwrap(), first);
    assert_eq!(store.len().unwrap(), count);
    c.expected_version_id = Some(first.version_id.clone());
    c.status = MemoryStatus::Superseded;
    c.replacement_version_id = Some(replacement.version_id.clone());
    let next = service.assess(&c).unwrap();
    assert_ne!(next.version_id, first.version_id);
    assert_eq!(
        service.current(&old.version_id).unwrap(),
        Some(next.clone())
    );
    assert!(matches!(
        service.assess(&command(&old.version_id)),
        Err(MemoryError::Conflict)
    ));
    assert_eq!(store.get(&old.version_id).unwrap(), Some(old.clone()));
    assert_eq!(
        store.get(&first.version_id).unwrap().unwrap().payload["status"],
        "active"
    );
    let links = store.outgoing_relations(&next.version_id).unwrap();
    for id in [&old.version_id, &replacement.version_id, &first.version_id] {
        assert!(links.iter().any(|link| link.to == *id && link.kind == crate::store::relation_kinds::based_on()));
    }
    assert!(service.current(&replacement.version_id).unwrap().is_none());
}

#[test]
fn time_bounds_are_inclusive_exclusive_and_disputed_is_explicitly_opted_in() {
    let store = SqliteArtifactStore::in_memory().unwrap();
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let note = source(&store, &schemas, "note");
    let service = MemoryAssessmentService {
        store: &store,
        schemas: &schemas,
        clock: &FixedClock,
    };
    let mut c = command(&note.version_id);
    c.status = MemoryStatus::Disputed;
    c.valid_from = Some(NOW.into());
    c.valid_until = Some("2027-02-18T12:00:00Z".into());
    service.assess(&c).unwrap();
    let cursor = store.latest_sequence().unwrap();
    for (at, disposition) in [
        ("2026-11-18T11:59:59Z", MemoryDisposition::NotYetValid),
        (NOW, MemoryDisposition::Disputed),
        ("2027-02-18T12:00:00Z", MemoryDisposition::Expired),
    ] {
        let result = service
            .evaluate(&note.version_id, cursor, at, &strict())
            .unwrap();
        assert_eq!(result.disposition, disposition);
        assert!(!result.admitted);
    }
    let relaxed = MemoryEpistemicPolicy {
        allow_unassessed: true,
        allow_disputed: true,
    };
    assert!(
        service
            .evaluate(&note.version_id, cursor, NOW, &relaxed)
            .unwrap()
            .admitted
    );
    assert!(
        !service
            .evaluate(&note.version_id, cursor, "2027-02-18T12:00:00Z", &relaxed)
            .unwrap()
            .admitted
    );
    let mut active = c.clone();
    active.expected_version_id = service
        .current(&note.version_id)
        .unwrap()
        .map(|a| a.version_id);
    active.status = MemoryStatus::Active;
    service.assess(&active).unwrap();
    assert!(
        service
            .evaluate(
                &note.version_id,
                store.latest_sequence().unwrap(),
                NOW,
                &strict()
            )
            .unwrap()
            .admitted
    );
}

#[test]
fn fixed_log_cursor_ignores_later_retraction_without_resurrecting_it_for_new_work() {
    let store = SqliteArtifactStore::in_memory().unwrap();
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let note = source(&store, &schemas, "note");
    let service = MemoryAssessmentService {
        store: &store,
        schemas: &schemas,
        clock: &FixedClock,
    };
    let absent = store.latest_sequence().unwrap();
    let mut c = command(&note.version_id);
    let first = service.assess(&c).unwrap();
    let cursor = store.latest_sequence().unwrap();
    c.expected_version_id = Some(first.version_id.clone());
    c.status = MemoryStatus::Retracted;
    service.assess(&c).unwrap();
    let prior = service
        .evaluate(&note.version_id, cursor, NOW, &strict())
        .unwrap();
    assert_eq!(prior.assessment_version_id, Some(first.version_id));
    assert!(prior.admitted);
    let fresh = service
        .evaluate(
            &note.version_id,
            store.latest_sequence().unwrap(),
            NOW,
            &strict(),
        )
        .unwrap();
    assert_eq!(fresh.disposition, MemoryDisposition::Retracted);
    assert!(!fresh.admitted);
    assert_eq!(
        service
            .evaluate(&note.version_id, absent, NOW, &strict())
            .unwrap()
            .disposition,
        MemoryDisposition::Unassessed
    );
}

#[test]
fn invalid_actor_dates_revisions_and_replacement_references_never_write() {
    let store = SqliteArtifactStore::in_memory().unwrap();
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let note = source(&store, &schemas, "note");
    let service = MemoryAssessmentService {
        store: &store,
        schemas: &schemas,
        clock: &FixedClock,
    };
    let base = command(&note.version_id);
    for date in [
        "2026-02-30T00:00:00Z",
        "2026-11-18T12:00:00+01:00",
        "2026-11-18T12:00:00.1Z",
        "+026-11-18T12:00:00Z",
    ] {
        let mut c = base.clone();
        c.valid_from = Some(date.into());
        assert!(service.assess(&c).is_err());
    }
    for actor in [
        ActorType::Model,
        ActorType::System,
        ActorType::Adapter,
        ActorType::Executor,
    ] {
        let mut c = base.clone();
        c.decision_by.actor_type = actor;
        assert!(service.assess(&c).is_err());
    }
    let mut c = base.clone();
    c.status = MemoryStatus::Superseded;
    assert!(service.assess(&c).is_err());
    c.replacement_version_id = Some(note.version_id.clone());
    assert!(service.assess(&c).is_err());
    c.replacement_version_id = Some(VersionId("missing".into()));
    assert!(service.assess(&c).is_err());
    let mut c = base.clone();
    c.evidence = vec![note.version_id.clone(); 2];
    assert!(service.assess(&c).is_err());
    let mut c = base.clone();
    c.valid_from = Some(NOW.into());
    c.valid_until = Some(NOW.into());
    assert!(service.assess(&c).is_err());
    assert_eq!(store.len().unwrap(), 1);
    let mut wrong_schema = source(&store, &schemas, "different");
    // Use an actual different persisted schema for the replacement check.
    wrong_schema.artifact_id = ArtifactId("candidate".into());
    wrong_schema.version_id = VersionId("candidate-v1".into());
    wrong_schema.schema_id = SchemaId("arcs.context_selection.v1".into());
    wrong_schema.artifact_type = "context_selection".into();
    wrong_schema.payload =
        json!({"request":{},"policy":{},"inputs":[],"selected":[],"trace":{},"skipped":0});
    store.append(&wrong_schema, &schemas).unwrap();
    let mut c = base;
    c.status = MemoryStatus::Superseded;
    c.replacement_version_id = Some(wrong_schema.version_id);
    assert!(service.assess(&c).is_err());
    let longest = source(&store, &schemas, &"x".repeat(509));
    assert_eq!(longest.version_id.0.len(), 512);
    let accepted = service.assess(&command(&longest.version_id)).unwrap();
    assert_eq!(
        service.current(&longest.version_id).unwrap(),
        Some(accepted)
    );
}

#[test]
fn untrusted_or_oversized_head_is_an_error_instead_of_unassessed_fallback() {
    let store = SqliteArtifactStore::in_memory().unwrap();
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let note = source(&store, &schemas, "note");
    let service = MemoryAssessmentService {
        store: &store,
        schemas: &schemas,
        clock: &FixedClock,
    };
    let first = service.assess(&command(&note.version_id)).unwrap();
    let mut forged = store.get(&first.version_id).unwrap().unwrap();
    forged.version = 2;
    forged.version_id = VersionId(format!("{}:v2", forged.artifact_id.0));
    forged.created_by.actor_type = ActorType::Model;
    store.append(&forged, &schemas).unwrap();
    assert!(service.current(&note.version_id).is_err());
    assert!(
        service
            .evaluate(
                &note.version_id,
                store.latest_sequence().unwrap(),
                NOW,
                &MemoryEpistemicPolicy {
                    allow_unassessed: true,
                    allow_disputed: true
                }
            )
            .is_err()
    );
    let scoped = store
        .schema_subject_head_at(
            &SchemaId(SCHEMA.into()),
            forged.subject.as_ref().unwrap(),
            store.latest_sequence().unwrap(),
            1,
        )
        .unwrap()
        .unwrap();
    assert!(scoped.artifact.is_none());
    assert!(
        store
            .schema_subject_head_at(
                &SchemaId(SCHEMA.into()),
                forged.subject.as_ref().unwrap(),
                u64::MAX,
                1
            )
            .is_err()
    );
}

#[test]
fn concurrent_connections_accept_one_revision_and_reopen_preserves_it() {
    use std::sync::{Arc, Barrier};
    let path = std::env::temp_dir().join(format!("arcs-memory-cas-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::open(&path).unwrap();
    let note = source(&store, &schemas, "note");
    // Pre-open SQLite connections so the barrier tests assessment CAS, not DDL.
    let connections: Vec<_> = (0..2)
        .map(|_| SqliteArtifactStore::open(&path).unwrap())
        .collect();
    let barrier = Arc::new(Barrier::new(2));
    let jobs: Vec<_> = connections
        .into_iter()
        .enumerate()
        .map(|(i, store)| {
            let schemas = schemas.clone();
            let mut c = command(&note.version_id);
            c.reason = format!("independent review {i}");
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                MemoryAssessmentService {
                    store: &store,
                    schemas: &schemas,
                    clock: &FixedClock,
                }
                .assess(&c)
            })
        })
        .collect();
    let results: Vec<_> = jobs.into_iter().map(|job| job.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .any(|r| matches!(r, Err(MemoryError::Conflict)))
    );
    drop(store);
    let reopened = SqliteArtifactStore::open(&path).unwrap();
    assert_eq!(reopened.len().unwrap(), 2);
    let current = MemoryAssessmentService {
        store: &reopened,
        schemas: &schemas,
        clock: &FixedClock,
    }
    .current(&note.version_id)
    .unwrap()
    .unwrap();
    assert_eq!(
        &current,
        results.iter().find_map(|r| r.as_ref().ok()).unwrap()
    );
    drop(reopened);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn failed_commit_rolls_back_revision_pointer_and_evidence_links() {
    let path = std::env::temp_dir().join(format!(
        "arcs-memory-rollback-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let store = SqliteArtifactStore::open(&path).unwrap();
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let note = source(&store, &schemas, "note");
    let service = MemoryAssessmentService {
        store: &store,
        schemas: &schemas,
        clock: &FixedClock,
    };
    let mut c = command(&note.version_id);
    let first = service.assess(&c).unwrap();
    c.expected_version_id = Some(first.version_id.clone());
    c.status = MemoryStatus::Retracted;
    let inject = rusqlite::Connection::open(&path).unwrap();
    inject.execute_batch("CREATE TRIGGER fail_memory_link BEFORE INSERT ON artifact_relations WHEN NEW.relation_kind='based_on' BEGIN SELECT RAISE(ABORT,'injected memory link failure'); END;").unwrap();
    let count = store.len().unwrap();
    assert!(matches!(service.assess(&c), Err(MemoryError::Store(_))));
    assert_eq!(store.len().unwrap(), count);
    assert_eq!(
        service.current(&note.version_id).unwrap(),
        Some(first.clone())
    );
    inject
        .execute_batch("DROP TRIGGER fail_memory_link")
        .unwrap();
    let next = service.assess(&c).unwrap();
    assert_ne!(next.version_id, first.version_id);
    assert_eq!(store.len().unwrap(), count + 1);
    drop(inject);
    drop(store);
    std::fs::remove_file(path).unwrap();
}
