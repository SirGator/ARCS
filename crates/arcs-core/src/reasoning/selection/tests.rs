use super::*;
use crate::core::{
    Actor, ActorType, Artifact, Source, SourceClass, SourceKind, SystemClock, Trust, TrustLevel,
};
use crate::reasoning::general::{GeneralReasoningService, ReasoningPreparation};
use crate::reasoning::test_support::Fixture;
use serde_json::json;

#[test]
fn a_model_cannot_forge_a_receipt_or_cause_a_reselection_over_it() {
    let (mut source, owner, policy) = fixture();
    let legitimate = selected(
        select(
            &mut source,
            &owner,
            &policy,
            &[ReasonerAvailability::Ready; 2],
        )
        .unwrap(),
    );
    let mut forged = source.store.get(&legitimate.version_id).unwrap().unwrap();
    forged.created_by.actor_type = ActorType::Model;
    forged.source.kind = SourceKind::External;
    forged.trust = Trust {
        level: TrustLevel::Low,
        source_class: SourceClass::Model,
    };
    let (mut target, _, _) = fixture();
    target.store.append(&forged, &target.schemas).unwrap();
    assert!(
        select(
            &mut target,
            &owner,
            &policy,
            &[ReasonerAvailability::Ready; 2]
        )
        .is_err()
    );
    let mut request = Fixture::request();
    request.constraints = json!({CONSTRAINT:forged.version_id});
    assert!(
        GeneralReasoningService {
            registry: &target.registry,
            schemas: &target.schemas,
            store: &target.store,
            ids: &mut target.ids,
            clock: &SystemClock
        }
        .prepare(&target.peer, request, &[])
        .is_err()
    );
}

fn fixture() -> (Fixture, SelectionOwner, SelectionPolicy) {
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
    let mut other = f.peer.manifest.clone();
    other.adapter_id = crate::adapters::AdapterId("other".into());
    let mut grant = Fixture::grant();
    grant.adapter_id = other.adapter_id.clone();
    f.registry
        .register(other, grant, &f.schemas, &f.store)
        .unwrap();
    let request = Fixture::request();
    let first = ReasonerChoice {
        capability: request.capability,
        method: request.method,
        budget: request.budget,
    };
    let mut second = first.clone();
    second.capability.adapter_id = crate::adapters::AdapterId("other".into());
    let policy = SelectionPolicy {
        target_schema_id: request.target_schema_id,
        query_text: String::new(),
        choices: vec![first, second],
    };
    let owner = SelectionOwner {
        definition_version: definition.version_id,
        input_version: VersionId("input-v1".into()),
        task_id: "1".into(),
    };
    (f, owner, policy)
}
fn select(
    f: &mut Fixture,
    owner: &SelectionOwner,
    policy: &SelectionPolicy,
    states: &[ReasonerAvailability],
) -> Result<SelectionPreparation, ReasoningError> {
    ReasonerSelectionService {
        registry: &f.registry,
        schemas: &f.schemas,
        store: &f.store,
        ids: &mut f.ids,
        clock: &SystemClock,
    }
    .select(owner, policy, states)
}
fn selected(result: SelectionPreparation) -> SelectedReasoner {
    let SelectionPreparation::Selected(value) = result else {
        panic!("expected selection");
    };
    *value
}

#[test]
fn waiting_consumes_no_binding_and_a_selected_alternative_survives_new_occupancy() {
    let (mut f, owner, policy) = fixture();
    let count = f.store.len().unwrap();
    assert!(matches!(
        select(&mut f, &owner, &policy, &[ReasonerAvailability::Busy; 2]).unwrap(),
        SelectionPreparation::Waiting
    ));
    assert_eq!(f.store.len().unwrap(), count);
    let chosen = selected(
        select(
            &mut f,
            &owner,
            &policy,
            &[ReasonerAvailability::Busy, ReasonerAvailability::Ready],
        )
        .unwrap(),
    );
    assert_eq!(chosen.choice().capability.adapter_id.0, "other");
    let replay = selected(
        select(
            &mut f,
            &owner,
            &policy,
            &[
                ReasonerAvailability::Ready,
                ReasonerAvailability::Unavailable,
            ],
        )
        .unwrap(),
    );
    assert_eq!(replay.version_id, chosen.version_id);
    assert_eq!(f.store.len().unwrap(), count + 1);
    let edges = crate::store::ArtifactRelations::new(&f.store)
        .outgoing(&chosen.version_id)
        .unwrap();
    assert!(edges.iter().any(|edge| edge.to == owner.definition_version));
    assert!(edges.iter().any(|edge| edge.to == owner.input_version));
    let mut changed = policy.clone();
    changed.choices.reverse();
    assert!(select(&mut f, &owner, &changed, &[ReasonerAvailability::Ready; 2]).is_err());
}

#[test]
fn every_choice_is_checked_and_no_match_or_untrusted_definition_cannot_fall_back() {
    let (mut f, owner, policy) = fixture();
    let count = f.store.len().unwrap();
    let mut invalid_policy = policy.clone();
    invalid_policy.choices[1].budget.max_runtime_ms = 1001;
    assert!(
        select(
            &mut f,
            &owner,
            &invalid_policy,
            &[ReasonerAvailability::Ready; 2]
        )
        .is_err()
    );
    invalid_policy = policy.clone();
    invalid_policy.query_text = "unknownpurpose".into();
    assert!(
        select(
            &mut f,
            &owner,
            &invalid_policy,
            &[ReasonerAvailability::Ready; 2]
        )
        .is_err()
    );
    let mut invalid_owner = owner.clone();
    invalid_owner.definition_version = owner.input_version.clone();
    assert!(
        select(
            &mut f,
            &invalid_owner,
            &policy,
            &[ReasonerAvailability::Ready; 2]
        )
        .is_err()
    );
    assert_eq!(f.store.len().unwrap(), count);
}

#[test]
fn selection_reopens_and_concurrent_writers_converge_on_one_choice() {
    let (mut f, owner, policy) = fixture();
    let directory = std::env::temp_dir().join(format!(
        "arcs-selection-race-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("core.sqlite");
    let store = SqliteArtifactStore::open(&path).unwrap();
    for artifact in f.store.committed_after(0).unwrap() {
        store.append(&artifact.artifact, &f.schemas).unwrap();
    }
    drop(store);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = (0..2)
        .map(|n| {
            let path = path.clone();
            let registry = f.registry.clone();
            let schemas = f.schemas.clone();
            let owner = owner.clone();
            let policy = policy.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let store = SqliteArtifactStore::open(&path).unwrap();
                let mut ids = store.durable_id_generator(&format!("writer{n}")).unwrap();
                let states = if n == 0 {
                    [ReasonerAvailability::Ready, ReasonerAvailability::Busy]
                } else {
                    [ReasonerAvailability::Busy, ReasonerAvailability::Ready]
                };
                barrier.wait();
                selected(
                    ReasonerSelectionService {
                        registry: &registry,
                        schemas: &schemas,
                        store: &store,
                        ids: &mut ids,
                        clock: &SystemClock,
                    }
                    .select(&owner, &policy, &states)
                    .unwrap(),
                )
            })
        })
        .collect::<Vec<_>>();
    let selections = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(selections[0].version_id, selections[1].version_id);
    f.store = SqliteArtifactStore::open(&path).unwrap();
    f.ids = f.store.durable_id_generator("reopened").unwrap();
    let reopened = selected(
        select(
            &mut f,
            &owner,
            &policy,
            &[ReasonerAvailability::Unavailable; 2],
        )
        .unwrap(),
    );
    assert_eq!(reopened.version_id, selections[0].version_id);
    drop(f);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_request_links_its_binding_without_disclosing_private_selection_data_and_rejects_budget_changes()
 {
    let (mut f, owner, mut policy) = fixture();
    policy.query_text = "peer".into();
    let selection =
        selected(select(&mut f, &owner, &policy, &[ReasonerAvailability::Ready; 2]).unwrap());
    let mut request = Fixture::request();
    request.constraints = json!({CONSTRAINT:selection.version_id});
    let prepared = GeneralReasoningService {
        registry: &f.registry,
        schemas: &f.schemas,
        store: &f.store,
        ids: &mut f.ids,
        clock: &SystemClock,
    }
    .prepare(&f.peer, request.clone(), &[])
    .unwrap();
    let ReasoningPreparation::Dispatch(prepared) = prepared else {
        panic!("expected dispatch");
    };
    assert_eq!(prepared.invocation().context.len(), 1);
    assert_eq!(
        prepared.invocation().context[0].payload,
        json!({"raw_text":"continue"})
    );
    let state = crate::runtime::InvocationService::new(&f.store, &f.schemas, &SystemClock)
        .lookup(&prepared.invocation().invocation_id)
        .unwrap()
        .unwrap();
    let audit = f.store.get(&state.input_version).unwrap().unwrap();
    assert!(
        audit
            .provenance
            .as_ref()
            .unwrap()
            .parents
            .contains(&selection.version_id.0)
    );
    assert!(
        crate::store::ArtifactRelations::new(&f.store)
            .outgoing(&audit.version_id)
            .unwrap()
            .iter()
            .any(|edge| edge.to == selection.version_id)
    );
    request.budget.max_runtime_ms = 500;
    assert!(
        GeneralReasoningService {
            registry: &f.registry,
            schemas: &f.schemas,
            store: &f.store,
            ids: &mut f.ids,
            clock: &SystemClock
        }
        .prepare(&f.peer, request, &[])
        .is_err()
    );
}
