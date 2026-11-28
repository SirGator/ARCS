//! Nachweis der Betriebsgrenze für externe Wirkungen.
//!
//! Diese Tests prüfen die Zuordnung einer externen Wirkung zu ihrer Aufgabe
//! und die ausdrückliche Klärung durch den Betreiber.

#[path = "support/goals.rs"]
mod goals;
mod support;

use arcs::core::SystemClock;
use arcs::runtime::effects::{EffectDraft, EffectService, EffectState};
use arcs_host::operator_command::{CommandReceipt, EffectCommand, OperatorCommand};
use serde_json::json;

fn clock() -> SystemClock {
    SystemClock
}

/// Legt eine ungeklärte Wirkung für eine Aufgabe an.
fn unresolved_effect(host: &arcs_host::Host, operation_id: &str, task_key: &str) -> String {
    let clock = clock();
    let service = EffectService::new(host.store(), host.schemas(), &clock);
    service
        .plan(&EffectDraft {
            operation_id: operation_id.to_string(),
            task_key: task_key.to_string(),
            kind: "act:simulated.fan".into(),
            invocation_id: format!("execution:{operation_id}"),
        })
        .unwrap();
    service.mark_dispatched(operation_id, None).unwrap();
    service
        .mark_unclear(operation_id, "the answer cannot be attributed")
        .unwrap();
    operation_id.to_string()
}

#[test]
fn an_operator_inspects_an_unresolved_effect_before_clarifying_it() {
    let f = goals::Fixture::new();
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let operation_id = unresolved_effect(&host, "operation:inspect", "goal:1");

    let receipt = OperatorCommand::Effect(EffectCommand::Inspect {
        operation_id: operation_id.clone(),
    })
    .apply(&mut runtime, &mut host)
    .unwrap()
    .expect("the inspection returns a receipt");
    let CommandReceipt::Effect(effect) = receipt else {
        panic!("expected an effect receipt");
    };
    assert_eq!(effect.state, "unclear");
    assert_eq!(effect.task_key, "goal:1");

    // A clarification without an operator or a reason is refused.
    let refused = OperatorCommand::Effect(EffectCommand::Reconcile {
        operation_id: operation_id.clone(),
        outcome: arcs_host::task_runtime::effects::EffectOutcome::DidNotOccur,
        reference: None,
        reason: "  ".into(),
        operator_id: "operator.test".into(),
    })
    .apply(&mut runtime, &mut host);
    assert!(refused.is_err());

    // A confirmed effect needs its reference.
    let refused = OperatorCommand::Effect(EffectCommand::Reconcile {
        operation_id: operation_id.clone(),
        outcome: arcs_host::task_runtime::effects::EffectOutcome::Occurred,
        reference: None,
        reason: "the device log shows the actuation".into(),
        operator_id: "operator.test".into(),
    })
    .apply(&mut runtime, &mut host);
    assert!(refused.is_err());

    // The explicit clarification decides and stays visible in the history.
    let receipt = OperatorCommand::Effect(EffectCommand::Reconcile {
        operation_id: operation_id.clone(),
        outcome: arcs_host::task_runtime::effects::EffectOutcome::DidNotOccur,
        reference: None,
        reason: "the device log shows no actuation".into(),
        operator_id: "operator.test".into(),
    })
    .apply(&mut runtime, &mut host)
    .unwrap()
    .expect("the clarification returns a receipt");
    let CommandReceipt::Effect(effect) = receipt else {
        panic!("expected an effect receipt");
    };
    assert_eq!(effect.state, "failed");
    assert!(
        effect
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("operator.test")),
        "{effect:?}"
    );

    // An effect that is already clarified cannot be clarified a second time.
    let refused = OperatorCommand::Effect(EffectCommand::Reconcile {
        operation_id,
        outcome: arcs_host::task_runtime::effects::EffectOutcome::DidNotOccur,
        reference: None,
        reason: "a second attempt to clarify".into(),
        operator_id: "operator.test".into(),
    })
    .apply(&mut runtime, &mut host);
    assert!(refused.is_err());
}

#[test]
fn a_clarified_effect_is_bound_to_its_task_and_keeps_its_history() {
    let f = goals::Fixture::new();
    let mut host = f.host();
    let mut runtime = f.runtime(&host);
    let operation_id = unresolved_effect(&host, "operation:history", "goal:7");

    OperatorCommand::Effect(EffectCommand::Reconcile {
        operation_id: operation_id.clone(),
        outcome: arcs_host::task_runtime::effects::EffectOutcome::Occurred,
        reference: Some("device-operation-42".into()),
        reason: "the device log shows the actuation".into(),
        operator_id: "operator.test".into(),
    })
    .apply(&mut runtime, &mut host)
    .unwrap();

    let clock = clock();
    let service = EffectService::new(host.store(), host.schemas(), &clock);
    let record = service.lookup(&operation_id).unwrap().unwrap();
    assert_eq!(record.state, EffectState::Confirmed);
    assert_eq!(record.task_key, "goal:7");
    assert_eq!(
        record.effect_reference.as_deref(),
        Some("device-operation-42")
    );

    // The history shows every state the effect passed through.
    let history = service.history(&operation_id).unwrap();
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
    let _ = json!({ "checked": true });
}
