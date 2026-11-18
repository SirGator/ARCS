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
        epistemic: None,
        anchors: vec![],
        scopes: vec![MemoryScope {
            filter: ArtifactFilter {
                schema_id: SchemaId(NOTE.into()),
                subject: None,
                tags: vec!["project:p".into()],
            },
            payload_fields: vec!["body".into()],
            text_query: None,
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
        .filter(|artifact| matches!(artifact.schema_id.0.as_str(), SCHEMA | EPISTEMIC_SCHEMA))
        .collect()
}

fn assess(
    f: &Fixture,
    target: &Artifact,
    status: crate::memory::MemoryStatus,
    until: Option<&str>,
    replacement: Option<&Artifact>,
) -> crate::memory::MemoryAssessment {
    use crate::memory::*;
    MemoryAssessmentService {
        store: &f.store,
        schemas: &f.schemas,
        clock: &SystemClock,
    }
    .assess(&MemoryAssessmentCommand {
        target_version_id: target.version_id.clone(),
        expected_version_id: None,
        decision_by: Actor {
            actor_type: ActorType::Human,
            id: "operator".into(),
        },
        reason: "PRIVATE_ASSESSMENT_REASON".into(),
        status,
        valid_from: None,
        valid_until: until.map(str::to_owned),
        evidence: vec![],
        replacement_version_id: replacement.map(|a| a.version_id.clone()),
    })
    .unwrap()
}

#[test]
fn epistemic_context_excludes_retracted_expired_disputed_and_unassessed_memories() {
    use crate::memory::*;
    let mut f = fixture();
    let active = note(&f, "active", "project:p", "usable older evidence");
    assess(&f, &active, MemoryStatus::Active, None, None);
    for (id, status, until) in [
        ("disputed", MemoryStatus::Disputed, None),
        ("retracted", MemoryStatus::Retracted, None),
        (
            "expired",
            MemoryStatus::Active,
            Some("2000-01-01T00:00:00Z"),
        ),
    ] {
        let target = note(&f, id, "project:p", "must not be automatically used");
        assess(&f, &target, status, until, None);
    }
    let unknown = note(&f, "unknown", "project:p", "unassessed proposal");
    let mut p = policy();
    p.epistemic = Some(MemoryEpistemicPolicy {
        allow_unassessed: false,
        allow_disputed: false,
    });
    let selected = augment(&mut f, request(), &p).unwrap();
    assert_eq!(selected.context.len(), 2);
    assert_eq!(selected.context[1].version_id, active.version_id);
    let saved = records(&f).remove(0);
    assert_eq!(saved.schema_id.0, EPISTEMIC_SCHEMA);
    assert_eq!(saved.payload["skipped"], 4);
    assert_eq!(
        saved.payload["epistemic"]["decisions"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    let invocation = match (GeneralReasoningService {
        registry: &f.registry,
        schemas: &f.schemas,
        store: &f.store,
        ids: &mut f.ids,
        clock: &SystemClock,
    })
    .prepare(&f.peer, selected, &[])
    .unwrap()
    {
        ReasoningPreparation::Dispatch(call) => call.invocation().clone(),
        _ => panic!("new invocation expected"),
    };
    let item = invocation
        .context
        .iter()
        .find(|item| item.version_id == active.version_id)
        .unwrap();
    assert_eq!(item.trust_level, TrustLevel::Low);
    let wire = serde_json::to_string(&invocation).unwrap();
    assert!(
        !wire.contains("PRIVATE_ASSESSMENT_REASON")
            && !wire.contains("private_note")
            && !wire.contains(&unknown.version_id.0)
    );
}

#[test]
fn retraction_changes_new_context_but_saved_request_replays_original_snapshot() {
    use crate::memory::*;
    let mut f = fixture();
    let target = note(&f, "old", "project:p", "original proposal");
    let receipt = assess(&f, &target, MemoryStatus::Active, None, None);
    let mut p = policy();
    p.epistemic = Some(MemoryEpistemicPolicy {
        allow_unassessed: false,
        allow_disputed: false,
    });
    let first = augment(&mut f, request(), &p).unwrap();
    let mut command = receipt.command;
    command.expected_version_id = Some(receipt.version_id);
    command.status = MemoryStatus::Retracted;
    MemoryAssessmentService {
        store: &f.store,
        schemas: &f.schemas,
        clock: &SystemClock,
    }
    .assess(&command)
    .unwrap();
    assert_eq!(augment(&mut f, request(), &p).unwrap(), first);
    let mut new = request();
    new.request_id.push_str("-new-round");
    assert_eq!(augment(&mut f, new, &p).unwrap().context.len(), 1);
    assert_eq!(records(&f).len(), 2);
}

#[test]
fn supersession_never_imports_a_replacement_from_an_unauthorized_scope() {
    use crate::memory::*;
    let mut f = fixture();
    let old = note(&f, "old", "project:p", "superseded");
    let foreign = note(&f, "foreign", "project:secret", "PRIVATE_REPLACEMENT_BODY");
    assess(&f, &old, MemoryStatus::Superseded, None, Some(&foreign));
    assess(&f, &foreign, MemoryStatus::Active, None, None);
    let mut p = policy();
    p.epistemic = Some(MemoryEpistemicPolicy {
        allow_unassessed: true,
        allow_disputed: true,
    });
    let selected = augment(&mut f, request(), &p).unwrap();
    assert_eq!(selected.context.len(), 1);
    let saved = records(&f).remove(0);
    assert!(
        !saved
            .payload
            .to_string()
            .contains("PRIVATE_REPLACEMENT_BODY")
    );
    assert_eq!(
        saved.payload["epistemic"]["decisions"][0]["disposition"],
        "superseded"
    );
    assert_eq!(saved.provenance.unwrap().parents.len(), 3); // input, excluded target, assessment
}

#[test]
fn unknown_and_disputed_memory_require_their_own_explicit_policy_opt_in() {
    use crate::memory::*;
    let mut f = fixture();
    let disputed = note(&f, "disputed", "project:p", "contested");
    assess(&f, &disputed, MemoryStatus::Disputed, None, None);
    note(&f, "unknown", "project:p", "unknown");
    let mut p = policy();
    p.epistemic = Some(MemoryEpistemicPolicy {
        allow_unassessed: false,
        allow_disputed: true,
    });
    let first = augment(&mut f, request(), &p).unwrap();
    assert_eq!(first.context.len(), 2);
    assert_eq!(first.context[1].version_id, disputed.version_id);
    p.epistemic = Some(MemoryEpistemicPolicy {
        allow_unassessed: true,
        allow_disputed: false,
    });
    let mut next = request();
    next.request_id.push_str("-different-policy");
    let next = augment(&mut f, next, &p).unwrap();
    assert_eq!(next.context.len(), 2);
    assert_eq!(next.context[1].version_id.0, "unknown-v1");
}

#[test]
fn later_months_use_fresh_validity_and_policy_changes_cannot_replace_a_request_snapshot() {
    use crate::memory::*;
    struct At(&'static str);
    impl Clock for At {
        fn now_rfc3339(&self) -> String {
            self.0.into()
        }
    }
    let mut f = fixture();
    let note = note(&f, "seasonal", "project:p", "temporary information");
    assess(
        &f,
        &note,
        MemoryStatus::Active,
        Some("2027-02-01T00:00:00Z"),
        None,
    );
    let mut p = policy();
    p.epistemic = Some(MemoryEpistemicPolicy {
        allow_unassessed: false,
        allow_disputed: false,
    });
    let mut at = |r, p: &MemoryPolicy, clock: &dyn Clock| {
        ReasoningContextService {
            registry: &f.registry,
            schemas: &f.schemas,
            store: &f.store,
            ids: &mut f.ids,
            clock,
        }
        .augment(r, p, &[])
    };
    let first = at(request(), &p, &At("2026-11-18T00:00:00Z")).unwrap();
    assert_eq!(first.context.len(), 2);
    assert_eq!(
        at(request(), &p, &At("2027-04-01T00:00:00Z")).unwrap(),
        first
    );
    let mut next = request();
    next.request_id.push_str("-next-months");
    assert_eq!(
        at(next, &p, &At("2027-04-01T00:00:00Z"))
            .unwrap()
            .context
            .len(),
        1
    );
    let mut different = p.clone();
    different.epistemic = None;
    assert!(at(request(), &different, &At("2027-04-01T00:00:00Z")).is_err());
    different.epistemic = Some(MemoryEpistemicPolicy {
        allow_unassessed: true,
        allow_disputed: true,
    });
    assert!(at(request(), &different, &At("2027-04-01T00:00:00Z")).is_err());
    assert_eq!(records(&f).len(), 2);
}

#[test]
fn text_context_finds_older_evidence_and_waits_without_freezing_partial_index() {
    let mut f = fixture();
    let wanted = note(&f, "wanted", "project:p", "confirmed battery calibration");
    for n in 0..70 {
        note(
            &f,
            &format!("noise-{n}"),
            "project:p",
            "unrelated recent note",
        );
    }
    note(
        &f,
        "foreign",
        "project:other",
        "confirmed battery calibration",
    );
    let mut p = policy();
    p.search.max_visited = 3;
    p.scopes[0].text_query = Some(MemoryTextQuery {
        text: "battery calibration".into(),
        input_fields: vec![],
    });
    for _ in 0..2 {
        assert!(matches!(
            augment(&mut f, request(), &p),
            Err(ReasoningError::Retrieval(
                crate::store::retrieval::RetrievalError::TextIndexPending { .. }
            ))
        ));
        assert!(records(&f).is_empty());
    }
    let selected = augment(&mut f, request(), &p).unwrap();
    assert_eq!(selected.context.len(), 2);
    assert_eq!(selected.context[1].version_id, wanted.version_id);
    assert_eq!(selected.context[1].payload_fields, vec!["body"]);
    let snapshot = records(&f).remove(0);
    assert_eq!(
        snapshot.payload["trace"]["text_indexes"][0]["skipped_oversized"],
        0
    );
    note(&f, "new", "project:p", "battery calibration new evidence");
    let count = f.store.len().unwrap();
    assert_eq!(augment(&mut f, request(), &p).unwrap(), selected);
    assert_eq!(f.store.len().unwrap(), count);
    assert_eq!(records(&f)[0].version_id, snapshot.version_id);
}

#[test]
fn hidden_memory_fields_cannot_make_a_public_projection_match() {
    let mut f = fixture();
    note(&f, "public", "project:p", "innocent text");
    let mut p = policy();
    p.scopes[0].text_query = Some(MemoryTextQuery {
        text: "must stay inside".into(),
        input_fields: vec![],
    });
    assert_eq!(augment(&mut f, request(), &p).unwrap().context.len(), 1);
    assert!(
        records(&f)[0].payload["selected"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn overlapping_scopes_export_only_fields_of_a_matching_text_scope() {
    let mut f = fixture();
    let wanted = note(
        &f,
        "calibration",
        "project:p",
        "verified battery calibration",
    );
    let mut p = policy();
    let mut public = p.scopes[0].clone();
    public.text_query = Some(MemoryTextQuery {
        text: "battery".into(),
        input_fields: vec![],
    });
    p.scopes[0].payload_fields = vec!["private_note".into()];
    p.scopes[0].text_query = Some(MemoryTextQuery {
        text: "unmatched".into(),
        input_fields: vec![],
    });
    p.scopes.push(public);
    let augmented = augment(&mut f, request(), &p).unwrap();
    assert_eq!(augmented.context[1].version_id, wanted.version_id);
    assert_eq!(augmented.context[1].payload_fields, vec!["body"]);
    assert_eq!(augment(&mut f, request(), &p).unwrap(), augmented);
}

#[test]
fn memory_query_fields_are_explicit_and_missing_or_empty_values_do_not_broaden_search() {
    for field in ["raw_text", "missing"] {
        let mut f = fixture();
        let wanted = note(&f, "wanted", "project:p", "continue the previous test");
        let mut p = policy();
        p.scopes[0].text_query = Some(MemoryTextQuery {
            text: String::new(),
            input_fields: vec![field.into()],
        });
        match augment(&mut f, request(), &p) {
            Ok(selected) => {
                assert_eq!(field, "raw_text");
                assert_eq!(selected.context[1].version_id, wanted.version_id);
            }
            Err(ReasoningError::InvalidReasoningRequest(_)) => {
                assert_eq!(field, "missing");
                assert!(records(&f).is_empty());
            }
            other => panic!("{other:?}"),
        }
    }
    let mut f = fixture();
    let mut p = policy();
    p.scopes[0].text_query = Some(MemoryTextQuery {
        text: "***".into(),
        input_fields: vec![],
    });
    assert!(augment(&mut f, request(), &p).is_err());
    assert!(records(&f).is_empty());
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
