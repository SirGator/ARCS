//! Wartegrenzen abgeschlossener Zielrunden; keine Unterbrechung laufender Effekte.
use super::{GoalConfig, GoalState, GoalTask, invalid};
use crate::task_runtime::TaskRuntime;
use crate::{Host, HostError};

impl TaskRuntime {
    pub(super) fn resume_waiting_goal(
        &self,
        host: &Host,
        task: &mut GoalTask,
    ) -> Result<bool, HostError> {
        let schedule = self
            .journal
            .current_schedule(task.sequence)?
            .ok_or_else(|| invalid("waiting goal is missing its activation receipt"))?;
        if schedule.released_at_unix_ms.is_none() {
            return Ok(false);
        }
        // The following checkpoint precedes fresh acquisition. Historical
        // candidates and approvals remain in the Core, without active authority.
        task.observation_cursor = host.store().latest_sequence().map_err(HostError::Store)?;
        task.wait_ticks = 0;
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
        task.dispatch_cursor = None;
        task.state = GoalState::Acquiring;
        Ok(true)
    }

    pub(super) fn persist_goal_step(
        &mut self,
        host: &Host,
        config: &GoalConfig,
        task: &GoalTask,
        now_unix_ms: u64,
    ) -> Result<(), HostError> {
        if matches!(task.state, GoalState::WaitingTime | GoalState::WaitingEvent) {
            let activation = config
                .between_rounds
                .as_ref()
                .ok_or_else(|| invalid("waiting goal has no configured round activation"))?;
            let cursor = host.store().latest_sequence().map_err(HostError::Store)?;
            return self
                .journal
                .wait_goal(task, activation, cursor, now_unix_ms);
        }
        self.save_goal(task)
    }
}
