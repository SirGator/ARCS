//! Shared session admission across managers; each installed port is exclusive.
use super::*;
impl Host {
    pub(in crate::runtime) fn busy_adapters(&self) -> HashSet<AdapterId> {
        self.reasoning_jobs
            .values()
            .flat_map(|job| job.resources.iter().cloned())
            .chain(
                self.request_jobs
                    .values()
                    .map(|job| job.adapter_id().clone()),
            )
            .chain(
                self.retiring_request_jobs
                    .iter()
                    .map(|j| j.report.capability.adapter_id.clone()),
            )
            .chain(self.reconnect_jobs.keys().cloned())
            .chain(self.controller_worker_adapters())
            .chain(self.act_workers.keys().cloned())
            .collect()
    }

    pub(in crate::runtime) fn ensure_adapter_available(
        &self,
        adapter: &AdapterId,
    ) -> Result<(), HostError> {
        if self.busy_adapters().contains(adapter) {
            return Err(HostError::AdapterBusy(adapter.clone()));
        }
        if self.unavailable_adapters.contains(adapter) {
            if self.reconnect_can_wait(adapter) {
                return Err(HostError::ResourceBusy {
                    resource: "reasoner-reconnect".into(),
                    requested: 1,
                    available: 0,
                });
            }
            return Err(HostError::Adapter {
                adapter: adapter.clone(),
                source: arcs::adapters::stdio::StdioError::Disconnected,
            });
        }
        Ok(())
    }
}
