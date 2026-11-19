//! Reconcile already dispatched work only after durable task-control changes.
use super::scheduler::now_unix_ms;
use super::*;
impl TaskRuntime {
    pub(super) fn reap_abandoned_task_work(&self, host: &mut Host) -> Result<(), HostError> {
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
