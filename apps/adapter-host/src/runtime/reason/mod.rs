//! Reason-Manager: kontrollierte synchrone und parallele Modellaufrufe.
mod cancellation;
mod jobs;
pub(super) use jobs::PendingReasoning;

use super::*;

impl Host {
    fn reasoning_key(request: &arcs::reasoning::ReasoningRequestV2) -> String {
        arcs::runtime::deterministic_invocation_id(
            arcs::runtime::InvocationKind::Reasoning,
            &[
                "v2",
                &request.capability.adapter_id.0,
                &request.capability.capability_id.0,
                &request.request_id,
            ],
        )
    }

    fn has_reasoning_result(&self, key: &str) -> Result<bool, HostError> {
        // Admission hint only. The Core still validates the complete request,
        // current grants, input fingerprint and cached artifacts before reuse.
        if self.resource_pools.is_empty() {
            return Ok(false);
        }
        arcs::runtime::InvocationService::new(&self.store, &self.schemas, &SystemClock)
            .lookup(key)
            .map(|state| {
                state
                    .is_some_and(|state| state.status == arcs::runtime::InvocationStatus::Succeeded)
            })
            .map_err(|e| HostError::Reasoning(arcs::reasoning::ReasoningError::Invocation(e)))
    }
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
        let cached = self.has_reasoning_result(&Self::reasoning_key(&request))?;
        if !cached {
            self.ensure_adapter_available(&request.capability.adapter_id)?;
            for input in inputs {
                self.ensure_adapter_available(&input.source.adapter_id)?;
            }
            let resources = std::iter::once(request.capability.adapter_id.clone())
                .chain(inputs.iter().map(|input| input.source.adapter_id.clone()))
                .collect();
            let claims = self.reasoning_claims(&resources)?;
            self.ensure_resources_available(&claims)?;
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
        let mut service = arcs::reasoning::general::GeneralReasoningService {
            registry: &self.registry,
            schemas: &self.schemas,
            store: &self.store,
            ids: &mut self.ids,
            clock: &SystemClock,
        };
        if cached {
            return match service
                .prepare(adapter.endpoint.as_ref(), request, inputs)
                .map_err(HostError::Reasoning)?
            {
                arcs::reasoning::general::ReasoningPreparation::Complete(result) => Ok(result),
                arcs::reasoning::general::ReasoningPreparation::Dispatch(_) => {
                    Err(invalid("cached reasoning state changed during admission"))
                }
            };
        }
        service
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
