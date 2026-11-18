//! Reason-Manager: kontrollierte synchrone und parallele Modellaufrufe.
mod cancellation;
mod jobs;
pub(super) use jobs::PendingReasoning;

use super::*;

impl Host {
    pub(crate) fn reasoner_choice_availability(
        &self,
        choice: &arcs::reasoning::selection::ReasonerChoice,
    ) -> Result<arcs::reasoning::selection::ReasonerAvailability, HostError> {
        use arcs::reasoning::selection::ReasonerAvailability;
        if !self
            .adapters
            .iter()
            .any(|adapter| adapter.config.grant.adapter_id == choice.capability.adapter_id)
            || self
                .unavailable_adapters
                .contains(&choice.capability.adapter_id)
        {
            return Ok(ReasonerAvailability::Unavailable);
        }
        if self.busy_adapters().contains(&choice.capability.adapter_id) {
            return Ok(ReasonerAvailability::Busy);
        }
        let claims = self.reasoning_claims(&std::collections::HashSet::from([choice
            .capability
            .adapter_id
            .clone()]))?;
        match self.ensure_resources_available(&claims) {
            Ok(()) => Ok(ReasonerAvailability::Ready),
            Err(HostError::ResourceBusy { .. }) => Ok(ReasonerAvailability::Busy),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn load_reasoner_selection(
        &self,
        owner: &arcs::reasoning::selection::SelectionOwner,
        policy: &arcs::reasoning::selection::SelectionPolicy,
    ) -> Result<Option<arcs::reasoning::selection::SelectedReasoner>, HostError> {
        arcs::reasoning::selection::load_selection(
            &self.registry,
            &self.schemas,
            &self.store,
            owner,
            policy,
        )
        .map_err(HostError::Reasoning)
    }

    pub(crate) fn has_reasoner_selection(
        &self,
        owner: &arcs::reasoning::selection::SelectionOwner,
    ) -> Result<bool, HostError> {
        arcs::reasoning::selection::selection_exists(&self.store, owner)
            .map_err(HostError::Reasoning)
    }

    pub(crate) fn select_reasoner(
        &mut self,
        owner: &arcs::reasoning::selection::SelectionOwner,
        policy: &arcs::reasoning::selection::SelectionPolicy,
    ) -> Result<arcs::reasoning::selection::SelectionPreparation, HostError> {
        // Load before looking at transient occupancy. Replay must not switch
        // providers because the old worker is busy or another one became free.
        if let Some(saved) = self.load_reasoner_selection(owner, policy)? {
            return Ok(arcs::reasoning::selection::SelectionPreparation::Selected(
                Box::new(saved),
            ));
        }
        let availability = policy
            .choices
            .iter()
            .map(|choice| self.reasoner_choice_availability(choice))
            .collect::<Result<Vec<_>, _>>()?;
        arcs::reasoning::selection::ReasonerSelectionService {
            registry: &self.registry,
            schemas: &self.schemas,
            store: &self.store,
            ids: &mut self.ids,
            clock: &SystemClock,
        }
        .select(owner, policy, &availability)
        .map_err(HostError::Reasoning)
    }

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
        .map_err(|error| match error {
            arcs::reasoning::ReasoningError::Retrieval(
                arcs::store::retrieval::RetrievalError::TextIndexPending {
                    indexed_through,
                    target,
                },
            ) => HostError::ResourceBusy {
                resource: "memory-text-index".into(),
                requested: target,
                available: indexed_through,
            },
            other => HostError::Reasoning(other),
        })
    }
}
