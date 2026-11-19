//! Resolve queued task routes from the bound operator definition and their origin.
use super::{ReasoningRoute, TaskRuntime};
use crate::{Host, HostError};

impl TaskRuntime {
    pub(super) fn route_for_job(
        &self,
        host: &Host,
        sequence: u64,
        id: &str,
    ) -> Result<ReasoningRoute, HostError> {
        let route = self.raw_route_for_job(host, sequence, id)?;
        self.apply_saved_reasoner(host, sequence, route)
    }

    pub(super) fn raw_route_for_job(
        &self,
        host: &Host,
        sequence: u64,
        id: &str,
    ) -> Result<ReasoningRoute, HostError> {
        if let Some(route) = self.config.routes.iter().find(|route| route.id == id) {
            return Ok(route.clone());
        }
        if id.starts_with("check:") {
            return self.recurring_route(sequence, id);
        }
        self.plan_step_route(host, sequence, id)
    }
}
