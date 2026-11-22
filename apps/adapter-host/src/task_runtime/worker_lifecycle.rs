//! Reconcile already dispatched work only after durable task-control changes.
use super::scheduler::now_unix_ms;
use super::*;
impl TaskRuntime {
    pub(super) fn reap_abandoned_task_work(&self, host: &mut Host) -> Result<(), HostError> {
        let mut abandoned_transforms = std::collections::HashSet::new();
        for sequence in host.dependency_transform_tasks(&self.journal.path) {
            if !self.journal.dependency_transform_runnable(sequence)? {
                abandoned_transforms.insert(sequence);
            }
        }
        host.release_dependency_transforms(&self.journal.path, &abandoned_transforms);
        let cancelled = self
            .journal
            .cancelled_reasoning(&host.pending_reasoning_request_ids())?;
        host.discard_reasoning_requests(&cancelled);
        let controls = self
            .journal
            .source_request_controls(&host.pending_request_inputs())?;
        host.discard_request_inputs(
            &controls
                .into_iter()
                .filter(|c| c.abandoned)
                .map(|c| c.input)
                .collect(),
        );
        for worker in host.finished_executions(&self.journal.path) {
            let still_executing = self.saved_goal(worker.sequence)?.is_some_and(|task| {
                task.state == goal::GoalState::Executing
                    && task.action.as_ref() == Some(&worker.action_version)
            });
            if still_executing && !self.journal.task_is_paused(worker.sequence)? {
                continue;
            }
            // A dispatched effect may already exist. Commit the receipt,
            // but leave the stopped or paused task's checkpoint as it is.
            if let Err(error) = host.collect_execution(&worker.capability.adapter_id)
                && is_core_failure(&error)
            {
                return Err(error);
            }
        }
        Ok(())
    }
    pub(super) fn collect_paused_source_requests(
        &mut self,
        host: &mut Host,
        has_deadlines: bool,
        report: &mut TickReport,
    ) -> Result<(), HostError> {
        let controls = self
            .journal
            .source_request_controls(&host.pending_request_inputs())?;
        for control in controls.into_iter().filter(|c| c.paused && !c.abandoned) {
            if has_deadlines && self.live_clock {
                self.journal.advance_deadline_clock(now_unix_ms()?)?;
            }
            if has_deadlines && self.expire_task(host, control.sequence, report)? {
                continue;
            }
            if let Err(error) = host.collect_request_input(&control.input) {
                let route = self.raw_route_for_job(host, control.sequence, &control.route)?;
                self.record_job_failure(control.sequence, &route, error, report)?;
            }
        }
        Ok(())
    }
}
