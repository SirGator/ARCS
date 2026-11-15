//! Reason-Manager: kontrollierte synchrone und parallele Modellaufrufe.
mod cancellation;
mod jobs;
pub(super) use jobs::PendingReasoning;

use super::*;

impl Host {
    pub fn reason(
        &mut self,
        request: arcs::reasoning::ReasoningRequestV2,
    ) -> Result<arcs::reasoning::general::ReasoningResult, HostError> {
        self.reason_with_data(request, &[])
    }
    pub fn reason_with_data(
        &mut self,
        request: arcs::reasoning::ReasoningRequestV2,
        inputs: &[arcs::adapters::input_data::InputDataSelection],
    ) -> Result<arcs::reasoning::general::ReasoningResult, HostError> {
        self.ensure_adapter_available(&request.capability.adapter_id)?;
        for input in inputs {
            self.ensure_adapter_available(&input.source.adapter_id)?;
        }
        let adapter = self
            .adapters
            .iter()
            .find(|adapter| adapter.config.grant.adapter_id == request.capability.adapter_id)
            .ok_or_else(|| invalid("unknown reasoning adapter"))?;
        let sources: Vec<&dyn arcs::adapters::data::DataAdapter> = self
            .adapters
            .iter()
            .map(|adapter| adapter.endpoint.as_ref() as &dyn arcs::adapters::data::DataAdapter)
            .collect();
        arcs::reasoning::general::GeneralReasoningService {
            registry: &self.registry,
            schemas: &self.schemas,
            store: &self.store,
            ids: &mut self.ids,
            clock: &SystemClock,
        }
        .reason_with_data(adapter.endpoint.as_ref(), request, inputs, &sources)
        .map_err(HostError::Reasoning)
    }
}

impl Host {
    pub(crate) fn recall_context(
        &mut self,
        request: arcs::reasoning::ReasoningRequestV2,
        policy: &arcs::reasoning::context::MemoryPolicy,
        inputs: &[arcs::adapters::input_data::InputDataSelection],
    ) -> Result<arcs::reasoning::ReasoningRequestV2, HostError> {
        arcs::reasoning::context::ReasoningContextService {
            registry: &self.registry,
            schemas: &self.schemas,
            store: &self.store,
            ids: &mut self.ids,
            clock: &SystemClock,
        }
        .augment(request, policy, inputs)
        .map_err(HostError::Reasoning)
    }
}
