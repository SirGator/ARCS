use super::*;
use crate::core::{SystemClock, TrustLevel};
use crate::reasoning::test_support::Fixture;
use std::sync::atomic::Ordering;

#[test]
fn preparation_performs_no_external_work_and_replay_needs_no_worker() {
    let mut f = Fixture::new();
    let prepared = f.dispatch();
    assert_eq!(f.peer.calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        InvocationService::new(&f.store, &f.schemas, &SystemClock)
            .lookup(&prepared.invocation.invocation_id)
            .unwrap()
            .unwrap()
            .status,
        InvocationStatus::Dispatched
    );
    let response = f.peer.propose_v2(prepared.invocation());
    let result = f
        .service()
        .finish(prepared, response.map_err(ReasoningError::AdapterCall))
        .unwrap();
    assert_eq!(result.candidates[0].trust.level, TrustLevel::Low);
    let count = f.store.len().unwrap();
    match f.prepare() {
        ReasoningPreparation::Complete(replay) => {
            assert_eq!(replay.result_version_id, result.result_version_id)
        }
        _ => panic!("cached result must not dispatch"),
    }
    assert_eq!(f.store.len().unwrap(), count);
    assert_eq!(f.peer.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn stale_worker_cannot_commit_after_dispatch_was_recovered() {
    let mut f = Fixture::new();
    let old = f.dispatch();
    let old_response = f.peer.propose_v2(old.invocation()).unwrap();
    let current = f.dispatch();
    assert!(
        matches!(f.service().finish(old,Ok(old_response.clone())),Err(ReasoningError::Invocation(crate::runtime::InvocationError::InvalidState(message))) if message.contains("stale"))
    );
    f.service().finish(current, Ok(old_response)).unwrap();
}

#[test]
fn worker_adopts_a_recorded_failure_and_explicit_retry_keeps_the_invocation_id() {
    let mut f = Fixture::new();
    let original = f.dispatch();
    let id = original.invocation().invocation_id.clone();
    assert!(matches!(
        f.service().finish(
            original,
            Err(ReasoningError::AdapterCall(AdapterCallError::Timeout))
        ),
        Err(ReasoningError::AdapterCall(AdapterCallError::Timeout))
    ));
    let mut service = GeneralReasoningService {
        registry: &f.registry,
        schemas: &f.schemas,
        store: &f.store,
        ids: &mut f.ids,
        clock: &SystemClock,
    };
    assert!(matches!(
        service.prepare_for_worker(&f.peer, Fixture::request(), &[]),
        Err(ReasoningError::RecordedFailure(_))
    ));
    let mut changed = Fixture::request();
    changed.objective = "changed request".into();
    assert!(matches!(
        service.prepare_for_worker(&f.peer, changed, &[]),
        Err(ReasoningError::Invocation(
            crate::runtime::InvocationError::IdentityConflict(_)
        ))
    ));
    let retry = f.dispatch();
    assert_eq!(retry.invocation().invocation_id, id);
    let response = f.peer.propose_v2(retry.invocation()).unwrap();
    f.service().finish(retry, Ok(response)).unwrap();
    assert_eq!(f.peer.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn revoked_grant_prevents_inflight_response_from_becoming_a_candidate() {
    let mut f = Fixture::new();
    let prepared = f.dispatch();
    let response = f.peer.propose_v2(prepared.invocation()).unwrap();
    let mut grant = Fixture::grant();
    grant.assigned_trust = TrustLevel::Low;
    let mut registry = AdapterRegistry::new();
    registry
        .register(f.peer.manifest.clone(), grant, &f.schemas, &f.store)
        .unwrap();
    f.registry = registry;
    assert!(
        matches!(f.service().finish(prepared,Ok(response)),Err(ReasoningError::InvalidReasoningRequest(message)) if message.contains("grant changed"))
    );
    assert!(
        f.store
            .committed_after(0)
            .unwrap()
            .iter()
            .all(|event| !event.artifact.artifact_type.ends_with("_candidate"))
    );
}
