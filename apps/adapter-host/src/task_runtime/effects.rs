//! Ausdrückliche Klärung ungeklärter externer Wirkungen.
//!
//! Eine ungeklärte Wirkung wird nicht wiederholt. Der Betreiber entscheidet
//! ausdrücklich, ob die Wirkung eingetreten ist oder nicht. Erst diese
//! Entscheidung löst die Aufgabe aus dem Wartezustand.

use arcs::core::SystemClock;
use arcs::runtime::effects::{EffectRecord, EffectResolution, EffectService, EffectState};
use serde::Serialize;

use crate::task_runtime::TaskRuntime;
use crate::{Host, HostError};

/// Wie der Betreiber eine ungeklärte Wirkung auflöst.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectOutcome {
    /// Die Wirkung ist eingetreten; ein Beleg ist erforderlich.
    Occurred,
    /// Die Wirkung ist nicht eingetreten; die Aufgabe darf neu versuchen.
    DidNotOccur,
}

/// Gemeinsame Uhr des Wirkungsbuchs.
static CLOCK: SystemClock = SystemClock;

/// Sichtbarer Stand einer externen Wirkung.
#[derive(Debug, Clone, Serialize)]
pub struct EffectReceipt {
    pub operation_id: String,
    pub state: String,
    /// Aufgabe, die die Wirkung verantwortet.
    pub task_key: String,
    /// Beleg der externen Wirkung, sobald sie bestätigt ist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Anzahl der Versuche.
    pub attempt: u32,
    /// Begründung oder Fehlertext.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl TaskRuntime {
    /// Liest den Stand einer externen Wirkung.
    pub fn effect(
        &self,
        host: &Host,
        operation_id: &str,
    ) -> Result<Option<EffectReceipt>, HostError> {
        let service = EffectService::new(host.store(), host.schemas(), &CLOCK);
        let record = service.lookup(operation_id).map_err(HostError::Effect)?;
        Ok(record.map(receipt))
    }

    /// Klärt eine ungeklärte externe Wirkung ausdrücklich.
    ///
    /// Die Klärung ist eine Entscheidung des Betreibers. Sie wird als neue
    /// Fassung des Eintrags festgehalten und ist damit nachvollziehbar.
    pub fn reconcile_effect(
        &mut self,
        host: &Host,
        operation_id: &str,
        resolution: EffectOutcome,
        reference: Option<&str>,
        reason: &str,
        operator_id: &str,
    ) -> Result<EffectReceipt, HostError> {
        let service = EffectService::new(host.store(), host.schemas(), &CLOCK);
        let record = service.lookup(operation_id).map_err(HostError::Effect)?;
        let Some(record) = record else {
            return Err(HostError::Effect(
                arcs::runtime::effects::EffectError::UnknownOperation(operation_id.to_string()),
            ));
        };
        if record.state != EffectState::Unclear {
            return Err(HostError::Effect(
                arcs::runtime::effects::EffectError::IllegalTransition {
                    from: record.state,
                    to: EffectState::Unclear,
                },
            ));
        }
        // The decision belongs to an operator and stays visible in the
        // history of the effect entry.
        let operator = operator_id.trim();
        if operator.is_empty() {
            return Err(HostError::Effect(
                arcs::runtime::effects::EffectError::Invalid(
                    "a clarification requires an operator".into(),
                ),
            ));
        }
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(HostError::Effect(
                arcs::runtime::effects::EffectError::Invalid(
                    "a clarification requires a reason".into(),
                ),
            ));
        }
        let reason = format!("operator {operator}: {reason}");
        let resolution = match resolution {
            EffectOutcome::Occurred => {
                let reference = reference
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        HostError::Effect(arcs::runtime::effects::EffectError::Invalid(
                            "a confirmed effect requires its reference".into(),
                        ))
                    })?;
                EffectResolution::Occurred {
                    reference: reference.to_string(),
                    reason,
                }
            }
            EffectOutcome::DidNotOccur => EffectResolution::DidNotOccur { reason },
        };
        // The clarification also releases the invocation of the effect. Only
        // then may the same task continue with the same action.
        let reconciled = service
            .reconcile_and_release(operation_id, &resolution)
            .map_err(HostError::Effect)?;
        // The clarification decides about the effect, not about the plan. The
        // task continues with the same action: the book of effects then allows
        // either a new attempt or only the recovery of the receipt.
        if let Some(sequence) = goal_sequence(&reconciled.task_key) {
            self.resume_goal_after_reconciliation(host, sequence, &reconciled)?;
        }
        Ok(receipt(reconciled))
    }
}

impl TaskRuntime {
    /// Gibt eine geparkte Aufgabe nach einer Klärung wieder frei.
    ///
    /// Die Klärung betrifft die Wirkung, nicht den Plan. Deshalb setzt die
    /// Aufgabe denselben Auftrag fort: Das Wirkungsbuch entscheidet danach,
    /// ob ein neuer Versuch erlaubt ist oder nur der Beleg der bereits
    /// eingetretenen Wirkung nachgeholt wird. Es entsteht weder eine zweite
    /// Wirkung noch eine zweite Planungsrunde.
    pub(super) fn resume_goal_after_reconciliation(
        &mut self,
        host: &Host,
        sequence: u64,
        effect: &EffectRecord,
    ) -> Result<(), HostError> {
        let Some(mut task) = self.saved_goal(sequence)? else {
            return Ok(());
        };
        if task.state != crate::task_runtime::goal::GoalState::Blocked {
            return Ok(());
        }
        if task.action.is_some() {
            // The task still carries the action that produced the effect. It
            // continues with it; the effect book guards the repetition.
            task.state = crate::task_runtime::goal::GoalState::Executing;
            task.detail = format!(
                "external effect {} was clarified; the task continues with the same action",
                effect.operation_id
            );
        } else {
            // The task never dispatched this effect. It plans again with fresh
            // evidence instead of repeating an unknown action.
            task.round = task.round.saturating_add(1);
            task.observation_cursor = host.store().latest_sequence().map_err(HostError::Store)?;
            task.source_cursors.clear();
            task.source_versions.clear();
            task.before.clear();
            task.after.clear();
            task.candidate = None;
            task.capability = None;
            task.verification = None;
            task.approval = None;
            task.action = None;
            task.execution = None;
            task.assessment = None;
            task.outcome = None;
            task.dispatch_cursor = None;
            task.state = crate::task_runtime::goal::GoalState::Acquiring;
            task.detail = format!(
                "external effect {} was clarified; the task plans again",
                effect.operation_id
            );
        }
        task.wait_ticks = 0;
        self.journal.resume_blocked_goal(&task)
    }
}

/// Leitet die Aufgabennummer aus dem Aufgabenschlüssel ab.
fn goal_sequence(task_key: &str) -> Option<u64> {
    task_key.strip_prefix("goal:")?.parse().ok()
}

fn receipt(record: EffectRecord) -> EffectReceipt {
    EffectReceipt {
        operation_id: record.operation_id,
        state: record.state.as_str().to_string(),
        task_key: record.task_key,
        reference: record.effect_reference,
        attempt: record.attempt,
        detail: record.detail,
    }
}
