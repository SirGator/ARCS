use super::*;

struct Time;
impl Clock for Time {
    fn now_rfc3339(&self) -> String {
        "2026-11-28T08:00:00Z".into()
    }
}

fn schemas() -> SchemaRegistry {
    SchemaRegistry::with_bundled_schemas().unwrap()
}

fn store() -> SqliteArtifactStore {
    SqliteArtifactStore::in_memory().unwrap()
}

fn draft() -> EffectDraft {
    EffectDraft {
        operation_id: "operation-1".into(),
        task_key: "goal:7".into(),
        kind: "act:shell".into(),
        invocation_id: "execution:abc".into(),
    }
}

#[test]
fn plans_an_effect_once_and_keeps_the_binding() {
    let schemas = schemas();
    let store = store();
    let service = EffectService::new(&store, &schemas, &Time);

    let planned = service.plan(&draft()).unwrap();
    assert_eq!(planned.state, EffectState::Planned);
    assert_eq!(planned.attempt, 0);

    let again = service.plan(&draft()).unwrap();
    assert_eq!(again.version_id, planned.version_id);

    let other = EffectDraft {
        invocation_id: "execution:other".into(),
        ..draft()
    };
    assert!(matches!(service.plan(&other), Err(EffectError::Invalid(_))));
}

#[test]
fn confirms_a_dispatched_effect_with_its_evidence() {
    let schemas = schemas();
    let store = store();
    let service = EffectService::new(&store, &schemas, &Time);

    service.plan(&draft()).unwrap();
    let dispatched = service.mark_dispatched("operation-1", None).unwrap();
    assert_eq!(dispatched.state, EffectState::Dispatched);
    assert_eq!(dispatched.attempt, 1);

    let confirmed = service.confirm("operation-1", "receipt:42").unwrap();
    assert_eq!(confirmed.state, EffectState::Confirmed);
    assert_eq!(confirmed.effect_reference.as_deref(), Some("receipt:42"));

    // A confirmed effect is not repeated. The next attempt only recovers its
    // missing receipt.
    let decision = service.authorize_dispatch("operation-1").unwrap();
    assert_eq!(
        decision,
        DispatchDecision::RecoverEvidence {
            reference: "receipt:42".into()
        }
    );
    let recovered = service.mark_evidence_recovery("operation-1").unwrap();
    assert_eq!(recovered.state, EffectState::Confirmed);
    assert_eq!(recovered.attempt, 2);
    assert_eq!(recovered.effect_reference.as_deref(), Some("receipt:42"));
}

#[test]
fn recovers_the_receipt_of_a_confirmed_effect_without_a_second_effect() {
    let schemas = schemas();
    let store = store();
    let service = EffectService::new(&store, &schemas, &Time);

    service.plan(&draft()).unwrap();
    service.mark_dispatched("operation-1", None).unwrap();
    service.confirm("operation-1", "receipt:42").unwrap();

    // Recovering the evidence keeps the effect confirmed. The attempt counter
    // shows that the call was repeated.
    let recovered = service.mark_evidence_recovery("operation-1").unwrap();
    assert_eq!(recovered.state, EffectState::Confirmed);
    assert_eq!(recovered.attempt, 2);
    assert_eq!(
        service.authorize_dispatch("operation-1").unwrap(),
        DispatchDecision::RecoverEvidence {
            reference: "receipt:42".into()
        }
    );

    // Only an unresolved effect may become uncertain.
    assert!(matches!(
        service.mark_unclear("operation-1", "too late"),
        Err(EffectError::IllegalTransition { .. })
    ));
    assert!(matches!(
        service.mark_failed("operation-1", "too late"),
        Err(EffectError::IllegalTransition { .. })
    ));
}

#[test]
fn refuses_a_second_dispatch_of_an_unresolved_effect() {
    let schemas = schemas();
    let store = store();
    let service = EffectService::new(&store, &schemas, &Time);

    service.plan(&draft()).unwrap();
    service.mark_dispatched("operation-1", None).unwrap();

    // Eine abgesendete Wirkung darf mit derselben idempotenten Sendung
    // wiederholt werden. Die Wiederholung erzeugt keine zweite Wirkung.
    let decision = service.authorize_dispatch("operation-1").unwrap();
    assert_eq!(decision, DispatchDecision::Replay { attempt: 1 });

    let replayed = service.mark_replayed("operation-1", None).unwrap();
    assert_eq!(replayed.state, EffectState::Dispatched);
    assert_eq!(replayed.attempt, 2);

    // Nur ein Ausgang, der sich keiner Wirkung zuordnen lässt, verlangt eine
    // ausdrückliche Klärung. Solange die Wirkung eindeutig zugeordnet ist,
    // darf keine Klärung stattfinden.
    assert!(matches!(
        service.reconcile(
            "operation-1",
            &EffectResolution::DidNotOccur {
                reason: "adapter confirmed no effect".into()
            }
        ),
        Err(EffectError::IllegalTransition { .. })
    ));

    service
        .mark_unclear("operation-1", "answer belongs to another operation")
        .unwrap();
    let decision = service.authorize_dispatch("operation-1").unwrap();
    assert!(matches!(
        decision,
        DispatchDecision::NeedsReconciliation { .. }
    ));

    // Erst die ausdrückliche Klärung öffnet den Weg erneut.
    let reconciled = service
        .reconcile(
            "operation-1",
            &EffectResolution::DidNotOccur {
                reason: "adapter confirmed no effect".into(),
            },
        )
        .unwrap();
    assert_eq!(reconciled.state, EffectState::Failed);
    assert_eq!(
        service.authorize_dispatch("operation-1").unwrap(),
        DispatchDecision::Send
    );
}

#[test]
fn reconciles_an_occurred_effect_with_its_reference() {
    let schemas = schemas();
    let store = store();
    let service = EffectService::new(&store, &schemas, &Time);

    service.plan(&draft()).unwrap();
    service.mark_dispatched("operation-1", None).unwrap();
    service
        .mark_unclear("operation-1", "response lost")
        .unwrap();

    let reconciled = service
        .reconcile(
            "operation-1",
            &EffectResolution::Occurred {
                reference: "receipt:late".into(),
                reason: "the remote system shows the receipt".into(),
            },
        )
        .unwrap();
    assert_eq!(reconciled.state, EffectState::Confirmed);
    assert_eq!(reconciled.effect_reference.as_deref(), Some("receipt:late"));
}

#[test]
fn rejects_illegal_transitions() {
    let schemas = schemas();
    let store = store();
    let service = EffectService::new(&store, &schemas, &Time);

    assert!(matches!(
        service.mark_dispatched("missing", None),
        Err(EffectError::UnknownOperation(_))
    ));

    service.plan(&draft()).unwrap();
    assert!(matches!(
        service.confirm("operation-1", "receipt:1"),
        Ok(record) if record.state == EffectState::Confirmed
    ));
    assert!(matches!(
        service.mark_failed("operation-1", "late failure"),
        Err(EffectError::IllegalTransition { .. })
    ));
}

#[test]
fn keeps_the_full_history_of_an_effect() {
    let schemas = schemas();
    let store = store();
    let service = EffectService::new(&store, &schemas, &Time);

    service.plan(&draft()).unwrap();
    service.mark_dispatched("operation-1", None).unwrap();
    service
        .mark_unclear("operation-1", "response lost")
        .unwrap();
    service
        .reconcile(
            "operation-1",
            &EffectResolution::Occurred {
                reference: "receipt:1".into(),
                reason: "the remote system shows the receipt".into(),
            },
        )
        .unwrap();

    let history = service.history("operation-1").unwrap();
    let states: Vec<EffectState> = history.iter().map(|record| record.state).collect();
    assert_eq!(
        states,
        vec![
            EffectState::Planned,
            EffectState::Dispatched,
            EffectState::Unclear,
            EffectState::Confirmed
        ]
    );
}

#[test]
fn rejects_unusable_operation_identifiers() {
    let schemas = schemas();
    let store = store();
    let service = EffectService::new(&store, &schemas, &Time);

    let empty = EffectDraft {
        operation_id: "  ".into(),
        ..draft()
    };
    assert!(matches!(service.plan(&empty), Err(EffectError::Invalid(_))));
}
