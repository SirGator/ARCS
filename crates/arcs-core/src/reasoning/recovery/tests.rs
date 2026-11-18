use super::*;
use crate::adapters::{AdapterCallError, AdapterId};
use crate::reasoning::{
    ReasoningStatus,
    general::{GeneralReasoningAdapter, PreparedReasoning, ReasoningPreparation},
    test_support::{Fixture, Peer},
};
use serde_json::json;
use std::sync::atomic::AtomicUsize;

fn fixture() -> (Fixture, Peer, Peer, RecoveryDefinition) {
    let mut f = Fixture::new();
    let definition = Artifact::new(
        "definition",
        "definition-v1",
        "input",
        "arcs.input.v1",
        "2026-11-18T00:00:00Z",
        Actor {
            actor_type: ActorType::System,
            id: "test.runtime".into(),
        },
        Source {
            kind: SourceKind::Internal,
            reference: "definition".into(),
        },
        Trust {
            level: TrustLevel::High,
            source_class: SourceClass::System,
        },
        "definition",
        json!({"raw_text":"operator policy"}),
    );
    f.store.append(&definition, &f.schemas).unwrap();
    let first = Peer {
        manifest: f.peer.manifest.clone(),
        calls: AtomicUsize::new(0),
    };
    let mut other = Peer {
        manifest: f.peer.manifest.clone(),
        calls: AtomicUsize::new(0),
    };
    other.manifest.adapter_id = AdapterId("other".into());
    let mut grant = Fixture::grant();
    grant.adapter_id = other.manifest.adapter_id.clone();
    f.registry
        .register(other.manifest.clone(), grant, &f.schemas, &f.store)
        .unwrap();
    let request = Fixture::request();
    let alternate = ReasonerChoice {
        capability: crate::adapters::CapabilityRef::new("other", "propose"),
        method: request.method,
        budget: request.budget.clone(),
    };
    let recovery = RecoveryDefinition {
        owner: SelectionOwner {
            definition_version: definition.version_id,
            input_version: VersionId("input-v1".into()),
            task_id: "1".into(),
        },
        base_request: request,
        inputs: vec![],
        policy: RecoveryPolicy {
            alternatives: vec![alternate],
            delay_ms: 100,
        },
    };
    (f, first, other, recovery)
}
fn prepare(
    f: &mut Fixture,
    definition: &RecoveryDefinition,
    now: u64,
) -> Result<RecoveryPreparation, ReasoningError> {
    RecoveryService {
        registry: &f.registry,
        schemas: &f.schemas,
        store: &f.store,
        ids: &mut f.ids,
        clock: &SystemClock,
    }
    .prepare(definition, now)
}
fn ready(value: RecoveryPreparation) -> RecoveryCall {
    match value {
        RecoveryPreparation::Ready(call) => *call,
        other => panic!("expected ready: {other:?}"),
    }
}
fn dispatch(f: &mut Fixture, peer: &Peer, call: &RecoveryCall) -> PreparedReasoning {
    match f
        .service()
        .prepare(peer, call.request.clone(), &[])
        .unwrap()
    {
        ReasoningPreparation::Dispatch(value) => *value,
        _ => panic!("expected dispatch"),
    }
}
fn fail(f: &mut Fixture, peer: &Peer, call: &RecoveryCall, error: AdapterCallError) {
    let prepared = dispatch(f, peer, call);
    assert!(
        f.service()
            .finish(prepared, Err(ReasoningError::AdapterCall(error)))
            .is_err()
    );
}

#[test]
fn an_evidenced_transient_failure_has_a_durable_delayed_alternative_and_no_same_step_retry() {
    let (mut f, first, other, definition) = fixture();
    let initial = ready(prepare(&mut f, &definition, 1000).unwrap());
    assert_eq!(initial.index, 0);
    fail(&mut f, &first, &initial, AdapterCallError::Timeout);
    assert!(
        f.service()
            .prepare(&first, initial.request.clone(), &[])
            .is_err()
    );
    assert!(matches!(
        prepare(&mut f, &definition, 1100).unwrap(),
        RecoveryPreparation::Waiting {
            not_before_unix_ms: 1200
        }
    ));
    assert!(matches!(
        prepare(&mut f, &definition, 1199).unwrap(),
        RecoveryPreparation::Waiting {
            not_before_unix_ms: 1200
        }
    ));
    let next = ready(prepare(&mut f, &definition, 1200).unwrap());
    assert_eq!(next.index, 1);
    assert_ne!(initial.request.request_id, next.request.request_id);
    assert_eq!(next.request.context, initial.request.context);
    assert_eq!(
        next.request.allowed_capabilities,
        initial.request.allowed_capabilities
    );
    let prepared = dispatch(&mut f, &other, &next);
    let response = other.propose_v2(prepared.invocation()).unwrap();
    let result = f.service().finish(prepared, Ok(response)).unwrap();
    assert_eq!(result.candidates[0].created_by.id, "other");
    assert_eq!(result.candidates[0].trust.level, TrustLevel::Low);
    let count = f.store.len().unwrap();
    let replay = ready(prepare(&mut f, &definition, 5000).unwrap());
    assert_eq!(next.step_version_id, replay.step_version_id);
    assert!(matches!(
        f.service().prepare(&other, replay.request, &[]).unwrap(),
        ReasoningPreparation::Complete(_)
    ));
    assert_eq!(f.store.len().unwrap(), count);
}

#[test]
fn valid_no_solution_and_permanent_failures_never_authorize_an_alternative() {
    for permanent in [true, false] {
        let (mut f, first, _, definition) = fixture();
        let initial = ready(prepare(&mut f, &definition, 1000).unwrap());
        if permanent {
            fail(
                &mut f,
                &first,
                &initial,
                AdapterCallError::Rejected("no permission in adapter".into()),
            );
            assert!(matches!(
                prepare(&mut f, &definition, 2000).unwrap(),
                RecoveryPreparation::Stopped { attempts: 1, .. }
            ));
        } else {
            let prepared = dispatch(&mut f, &first, &initial);
            let mut response = first.propose_v2(prepared.invocation()).unwrap();
            response.status = ReasoningStatus::NoSolution;
            response.candidates.clear();
            f.service().finish(prepared, Ok(response)).unwrap();
            let call = ready(prepare(&mut f, &definition, 2000).unwrap());
            assert_eq!(call.index, 0);
        }
        assert_eq!(
            f.store
                .committed_after(0)
                .unwrap()
                .iter()
                .filter(|e| e.artifact.schema_id.0 == STEP_SCHEMA)
                .count(),
            1
        );
    }
}

#[test]
fn recovery_request_and_policy_cannot_change_context_budget_authority_or_input_sources() {
    let (mut f, first, _, definition) = fixture();
    let initial = ready(prepare(&mut f, &definition, 1000).unwrap());
    for field in [
        "objective",
        "budget",
        "capability",
        "context",
        "constraints",
    ] {
        let mut request = initial.request.clone();
        match field {
            "objective" => request.objective = "different".into(),
            "budget" => request.budget.max_runtime_ms -= 1,
            "capability" => request.capability.adapter_id = AdapterId("other".into()),
            "context" => request.context[0].payload_fields.clear(),
            _ => {
                request.constraints["new_authority"] = json!(true);
            }
        }
        assert!(request_recovery(&f.registry, &f.schemas, &f.store, &request, &[]).is_err());
    }
    let mut changed = definition.clone();
    let extra = InputDataSelection {
        name: "secret-frame".into(),
        source: crate::adapters::CapabilityRef::new("data", "read"),
        source_version_id: VersionId("input-v1".into()),
        reference_pointer: "/frame".into(),
    };
    assert!(
        request_recovery(
            &f.registry,
            &f.schemas,
            &f.store,
            &initial.request,
            &[extra]
        )
        .is_err()
    );
    changed.policy.delay_ms = 0;
    assert!(prepare(&mut f, &changed, 1000).is_err());
    fail(&mut f, &first, &initial, AdapterCallError::Timeout);
    let audit = f
        .store
        .get(
            &validation::load(&f.registry, &f.schemas, &f.store, &initial.step_version_id)
                .unwrap()
                .1
                .definition_version_id,
        )
        .unwrap()
        .unwrap();
    let fake = RecoveryStep {
        definition_version_id: audit.version_id,
        index: 1,
        previous_step: Some(initial.step_version_id),
        failure_version_id: Some(VersionId("input-v1".into())),
        not_before_unix_ms: 2000,
    };
    let artifact = records::persist_step(
        &mut RecoveryService {
            registry: &f.registry,
            schemas: &f.schemas,
            store: &f.store,
            ids: &mut f.ids,
            clock: &SystemClock,
        },
        &fake,
    )
    .unwrap();
    assert!(validation::load(&f.registry, &f.schemas, &f.store, &artifact.version_id).is_err());
}

#[test]
fn eight_completed_requests_exhaust_the_chain_and_repeated_ticks_cannot_extend_it() {
    let (mut f, first, other, mut definition) = fixture();
    definition.policy.delay_ms = 0;
    definition.policy.alternatives = vec![definition.policy.alternatives[0].clone(); 7];
    for index in 0..8 {
        let call = ready(prepare(&mut f, &definition, 1000).unwrap());
        assert_eq!(call.index, index);
        fail(
            &mut f,
            if index == 0 { &first } else { &other },
            &call,
            AdapterCallError::Unavailable("disconnected".into()),
        );
    }
    let count = f.store.len().unwrap();
    for now in [1001, 10000, u32::MAX as u64] {
        assert!(matches!(
            prepare(&mut f, &definition, now).unwrap(),
            RecoveryPreparation::Stopped { attempts: 8, .. }
        ));
    }
    assert_eq!(f.store.len().unwrap(), count);
}

#[test]
fn an_unknown_dispatch_reuses_the_original_identity_without_claiming_failure_or_choosing_again() {
    let (mut f, first, _, definition) = fixture();
    let initial = ready(prepare(&mut f, &definition, 1000).unwrap());
    let prepared = dispatch(&mut f, &first, &initial);
    drop(prepared);
    let replay = ready(prepare(&mut f, &definition, 2000).unwrap());
    assert_eq!(replay.index, 0);
    assert_eq!(initial.step_version_id, replay.step_version_id);
    assert_eq!(initial.request, replay.request);
    assert!(
        !f.store
            .committed_after(0)
            .unwrap()
            .iter()
            .any(|e| e.artifact.schema_id.0 == super::super::attempts::SCHEMA)
    );
}

#[test]
fn independent_connections_converge_and_reopening_preserves_the_first_due_time() {
    let (mut f, first, _, definition) = fixture();
    let directory = std::env::temp_dir().join(format!(
        "arcs-recovery-race-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("core.sqlite");
    let store = SqliteArtifactStore::open(&path).unwrap();
    for entry in f.store.committed_after(0).unwrap() {
        store.append(&entry.artifact, &f.schemas).unwrap();
    }
    f.store = store;
    f.ids = f.store.durable_id_generator("race").unwrap();
    let initial = ready(prepare(&mut f, &definition, 1000).unwrap());
    fail(&mut f, &first, &initial, AdapterCallError::Timeout);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = (0..2)
        .map(|index| {
            let path = path.clone();
            let registry = f.registry.clone();
            let schemas = f.schemas.clone();
            let definition = definition.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let store = SqliteArtifactStore::open(path).unwrap();
                let mut ids = store
                    .durable_id_generator(&format!("writer{index}"))
                    .unwrap();
                barrier.wait();
                RecoveryService {
                    registry: &registry,
                    schemas: &schemas,
                    store: &store,
                    ids: &mut ids,
                    clock: &SystemClock,
                }
                .prepare(&definition, 1100 + index)
                .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let due = threads
        .into_iter()
        .map(|thread| match thread.join().unwrap() {
            RecoveryPreparation::Waiting { not_before_unix_ms } => not_before_unix_ms,
            other => panic!("{other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(due[0], due[1]);
    f.store = SqliteArtifactStore::open(&path).unwrap();
    f.ids = f.store.durable_id_generator("reopen").unwrap();
    let call = ready(prepare(&mut f, &definition, due[0]).unwrap());
    assert_eq!(call.index, 1);
    drop(f);
    std::fs::remove_dir_all(directory).unwrap();
}
