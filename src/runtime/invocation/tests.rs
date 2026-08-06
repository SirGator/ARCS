use crate::core::{Clock, SchemaRegistry};
use crate::store::SqliteArtifactStore;

use super::*;

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2026-08-06T12:00:00Z".into()
    }
}

#[test]
fn prepared_invocation_is_recovered_from_current_state() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let service = InvocationService::new(&store, &schemas, &FixedClock);
    let spec = InvocationSpec {
        invocation_id: deterministic_invocation_id(InvocationKind::Request, &["a", "b"]),
        kind: InvocationKind::Request,
        capability: "adapter/capability".into(),
        input_version: crate::core::VersionId("input-v1".into()),
    };
    let prepared = service.prepare(spec.clone()).unwrap();
    let loaded = service.prepare(spec).unwrap();

    assert_eq!(prepared, loaded);
    assert_eq!(loaded.status, InvocationStatus::Prepared);
    assert_eq!(store.len().unwrap(), 1);
}

#[test]
fn dispatched_invocation_is_marked_unknown_then_redispatched() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let service = InvocationService::new(&store, &schemas, &FixedClock);
    let prepared = service
        .prepare(InvocationSpec {
            invocation_id: "request:recovery".into(),
            kind: InvocationKind::Request,
            capability: "adapter/capability".into(),
            input_version: crate::core::VersionId("input-v1".into()),
        })
        .unwrap();
    let dispatched = service.dispatch(&prepared).unwrap();

    let reopened = InvocationService::new(&store, &schemas, &FixedClock);
    let persisted = reopened.lookup("request:recovery").unwrap().unwrap();
    let recovered = reopened.recover(&persisted).unwrap();
    let redispatched = reopened.dispatch(&recovered).unwrap();

    assert_eq!(recovered.status, InvocationStatus::Unknown);
    assert_eq!(redispatched.status, InvocationStatus::Dispatched);
    assert_eq!(redispatched.invocation_id, dispatched.invocation_id);
}

#[test]
fn only_retryable_failure_returns_to_prepared() {
    let schemas = SchemaRegistry::with_bundled_schemas().unwrap();
    let store = SqliteArtifactStore::in_memory().unwrap();
    let service = InvocationService::new(&store, &schemas, &FixedClock);

    let retryable = service
        .prepare(InvocationSpec {
            invocation_id: "request:retryable".into(),
            kind: InvocationKind::Request,
            capability: "adapter/retryable".into(),
            input_version: crate::core::VersionId("input-1".into()),
        })
        .unwrap();
    let retryable = service.dispatch(&retryable).unwrap();
    let retryable = service.fail(&retryable, "timeout", true).unwrap();
    let reopened = InvocationService::new(&store, &schemas, &FixedClock);
    let retryable = reopened.lookup(&retryable.invocation_id).unwrap().unwrap();
    assert!(retryable.retryable);
    assert_eq!(
        reopened.recover(&retryable).unwrap().status,
        InvocationStatus::Prepared
    );

    let invalid = service
        .prepare(InvocationSpec {
            invocation_id: "request:invalid".into(),
            kind: InvocationKind::Request,
            capability: "adapter/invalid".into(),
            input_version: crate::core::VersionId("input-3".into()),
        })
        .unwrap();
    assert!(matches!(
        service.fail(&invalid, "not dispatched", false),
        Err(InvocationError::InvalidTransition {
            from: InvocationStatus::Prepared,
            to: InvocationStatus::Failed,
        })
    ));

    let permanent = service
        .prepare(InvocationSpec {
            invocation_id: "request:permanent".into(),
            kind: InvocationKind::Request,
            capability: "adapter/permanent".into(),
            input_version: crate::core::VersionId("input-2".into()),
        })
        .unwrap();
    let permanent = service.dispatch(&permanent).unwrap();
    let permanent = service.fail(&permanent, "protocol", false).unwrap();
    assert!(matches!(
        service.recover(&permanent),
        Err(InvocationError::NotRunnable(InvocationStatus::Failed))
    ));
}
