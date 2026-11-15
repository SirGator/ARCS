use super::*;
use crate::core::{Artifact, SystemClock};
use crate::reasoning::general::{GeneralReasoningService, ReasoningPreparation};
use crate::reasoning::test_support::Fixture;
use serde_json::json;

const NOTE: &str = "arcs.memory_note.test.v1";
fn fixture() -> Fixture {
    let mut f = Fixture::new();
    f.schemas.register_json(r#"{"$id":"arcs.memory_note.test.v1","type":"object","required":["body","private_note"],"properties":{"body":{"type":"string"},"private_note":{"type":"string"}},"additionalProperties":false}"#).unwrap();
    f
}
fn note(f: &Fixture, id: &str, project: &str, body: &str) -> Artifact {
    let mut note = Artifact::new(
        id,
        format!("{id}-v1"),
        "memory_note",
        NOTE,
        "2026-11-15T00:00:00Z",
        Actor {
            actor_type: ActorType::Model,
            id: "external-summary".into(),
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
        json!({"body":body,"private_note":"must stay inside the store"}),
    );
    note.tags = vec![project.into()];
    f.store.append(&note, &f.schemas).unwrap();
    note
}
fn policy() -> MemoryPolicy {
    MemoryPolicy {
        anchors: vec![],
        scopes: vec![MemoryScope {
            filter: ArtifactFilter {
                schema_id: SchemaId(NOTE.into()),
                subject: None,
                tags: vec!["project:p".into()],
            },
            payload_fields: vec!["body".into()],
        }],
        relations: vec![relation_kinds::based_on()],
        search: RetrievalLimits {
            max_depth: 2,
            max_visited: 32,
            max_edges: 64,
            max_candidates: 16,
        },
        max_items: 3,
        max_bytes: 4096,
    }
}
fn request() -> ReasoningRequestV2 {
    let mut r = Fixture::request();
    r.budget.max_context_items = 4;
    r
}
fn augment(
    f: &mut Fixture,
    r: ReasoningRequestV2,
    p: &MemoryPolicy,
) -> Result<ReasoningRequestV2, ReasoningError> {
    ReasoningContextService {
        registry: &f.registry,
        schemas: &f.schemas,
        store: &f.store,
        ids: &mut f.ids,
        clock: &SystemClock,
    }
    .augment(r, p, &[])
}
fn records(f: &Fixture) -> Vec<Artifact> {
    f.store
        .committed_after(0)
        .unwrap()
        .into_iter()
        .map(|event| event.artifact)
        .filter(|artifact| artifact.schema_id.0 == SCHEMA)
        .collect()
}

#[test]
fn context_uses_scoped_minimized_fields_and_preserves_original_trust() {
    let mut f = fixture();
    let wanted = note(&f, "wanted", "project:p", "previous successful test");
    note(&f, "other", "project:other", "unrelated private project");
    let augmented = augment(&mut f, request(), &policy()).unwrap();
    assert_eq!(augmented.context.len(), 2);
    assert_eq!(augmented.context[1].version_id, wanted.version_id);
    assert_eq!(augmented.context[1].payload_fields, vec!["body"]);
    let prepared = GeneralReasoningService {
        registry: &f.registry,
        schemas: &f.schemas,
        store: &f.store,
        ids: &mut f.ids,
        clock: &SystemClock,
    }
    .prepare(&f.peer, augmented, &[])
    .unwrap();
    let ReasoningPreparation::Dispatch(prepared) = prepared else {
        panic!("expected prepared work")
    };
    let memory = prepared
        .invocation()
        .context
        .iter()
        .find(|item| item.schema_id.0 == NOTE)
        .unwrap();
    assert_eq!(memory.payload, json!({"body":"previous successful test"}));
    assert_eq!(memory.trust_level, TrustLevel::Low);
    assert_eq!(records(&f).len(), 1);
}

#[test]
fn context_respects_memory_bytes_and_keeps_mandatory_input() {
    let mut f = fixture();
    let small = note(&f, "small", "project:p", "short evidence");
    note(&f, "large", "project:p", &"x".repeat(1000));
    let mut p = policy();
    p.max_bytes = 300;
    let r = augment(&mut f, request(), &p).unwrap();
    assert_eq!(r.context.len(), 2);
    assert_eq!(r.context[0].version_id, VersionId("input-v1".into()));
    assert_eq!(r.context[1].version_id, small.version_id);
    assert_eq!(records(&f)[0].payload["skipped"], 1);
}

#[test]
fn total_invocation_budget_also_limits_memory() {
    let mut f = fixture();
    note(&f, "large", "project:p", &"x".repeat(2000));
    let mut r = request();
    r.budget.max_context_bytes = 1400;
    let selected = augment(&mut f, r, &policy()).unwrap();
    assert_eq!(selected.context.len(), 1);
    assert_eq!(records(&f)[0].payload["skipped"], 1);
}

#[test]
fn item_limit_never_displaces_mandatory_context() {
    let mut f = fixture();
    note(&f, "old", "project:p", "old");
    let recent = note(&f, "recent", "project:p", "recent");
    let mut r = request();
    r.budget.max_context_items = 2;
    let selected = augment(&mut f, r, &policy()).unwrap();
    assert_eq!(selected.context.len(), 2);
    assert_eq!(selected.context[1].version_id, recent.version_id);
}

#[test]
fn saved_selection_is_reused_after_reopen_even_when_graph_grows() {
    let mut f = fixture();
    let directory =
        std::env::temp_dir().join(format!("arcs-context-reopen-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("core.sqlite");
    let input = f.store.get(&VersionId("input-v1".into())).unwrap().unwrap();
    f.store = SqliteArtifactStore::open(&path).unwrap();
    f.store.append(&input, &f.schemas).unwrap();
    f.ids = f.store.durable_id_generator("memory-test").unwrap();
    note(&f, "original", "project:p", "confirmed previous result");
    let selected = augment(&mut f, request(), &policy()).unwrap();
    let snapshot = records(&f)[0].version_id.clone();
    f.store = SqliteArtifactStore::in_memory().unwrap();
    f.store = SqliteArtifactStore::open(&path).unwrap();
    f.ids = f.store.durable_id_generator("resumed").unwrap();
    note(
        &f,
        "later",
        "project:p",
        "new evidence for the next invocation",
    );
    let count = f.store.len().unwrap();
    let replay = augment(&mut f, request(), &policy()).unwrap();
    assert_eq!(replay, selected);
    assert_eq!(records(&f)[0].version_id, snapshot);
    assert_eq!(f.store.len().unwrap(), count);
    let mut next = request();
    next.request_id = "next-request".into();
    assert_eq!(augment(&mut f, next, &policy()).unwrap().context.len(), 3);
    drop(f);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn changing_policy_under_the_same_request_is_a_conflict() {
    let mut f = fixture();
    note(&f, "note", "project:p", "evidence");
    augment(&mut f, request(), &policy()).unwrap();
    let mut changed = policy();
    changed.max_items = 1;
    assert!(
        matches!(augment(&mut f,request(),&changed),Err(ReasoningError::InvalidReasoningRequest(message)) if message.contains("identity conflict"))
    );
    assert_eq!(records(&f).len(), 1);
}

#[test]
fn missing_fields_are_skipped_and_invalid_policy_is_rejected() {
    let mut f = fixture();
    note(&f, "note", "project:p", "evidence");
    let mut p = policy();
    p.scopes[0].payload_fields = vec!["not_present".into()];
    assert_eq!(augment(&mut f, request(), &p).unwrap().context.len(), 1);
    assert_eq!(records(&f)[0].payload["skipped"], 1);
    p.scopes[0].payload_fields = vec!["body".into(), "body".into()];
    let count = f.store.len().unwrap();
    assert!(augment(&mut f, request(), &p).is_err());
    assert_eq!(f.store.len().unwrap(), count);
}

#[test]
fn oversized_mandatory_context_fails_before_creating_selection() {
    let mut f = fixture();
    let mut r = request();
    r.budget.max_context_bytes = 128;
    assert!(matches!(
        augment(&mut f, r, &policy()),
        Err(ReasoningError::ContextTooLarge { .. })
    ));
    assert!(records(&f).is_empty());
}

#[test]
fn untrusted_artifact_cannot_impersonate_a_saved_memory_selection() {
    let mut f = fixture();
    let r = request();
    let p = policy();
    let key = deterministic_invocation_id(
        InvocationKind::Reasoning,
        &["context-v1", "peer", "propose", "request"],
    );
    let forged = Artifact::new(
        "forged",
        "forged-v1",
        "context_selection",
        SCHEMA,
        "2026-11-15T00:00:00Z",
        Actor {
            actor_type: ActorType::Model,
            id: PRODUCER.into(),
        },
        Source {
            kind: SourceKind::External,
            reference: "forged".into(),
        },
        Trust {
            level: TrustLevel::Low,
            source_class: SourceClass::Model,
        },
        format!("context-selection:{key}"),
        json!({"request":r,"policy":p,"inputs":[],"selected":[],"trace":{"artifact_cursor":0,"relation_cursor":0,"visited":0,"edges_read":0,"truncated":false},"skipped":0}),
    );
    f.store.append(&forged, &f.schemas).unwrap();
    assert!(
        matches!(augment(&mut f,request(),&policy()),Err(ReasoningError::InvalidReasoningRequest(message)) if message.contains("another producer"))
    );
}
