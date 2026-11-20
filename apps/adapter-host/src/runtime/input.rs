//! Input-Manager: begrenzter Empfang, persistenter Core-Ingress und ACK.
use super::*;

impl Host {
    pub fn poll_inputs(&mut self) -> Result<Vec<Artifact>, HostError> {
        let mut received = Vec::new();
        let busy = self.busy_adapters();
        for adapter in &mut self.adapters {
            if busy.contains(&adapter.config.grant.adapter_id)
                || self
                    .unavailable_adapters
                    .contains(&adapter.config.grant.adapter_id)
            {
                continue;
            }
            if !adapter.endpoint.manifest().capabilities.iter().any(|cap| {
                cap.contract.is_input()
                    && adapter.config.grant.enabled_capabilities.contains(&cap.id)
            }) {
                continue;
            }
            let mut idle = false;
            let mut inputs = InputService::new(
                &self.registry,
                &self.schemas,
                &self.store,
                &mut self.ids,
                &SystemClock,
            );
            for _ in 0..self.polling.max_inputs_per_adapter {
                let Some(input) = adapter.endpoint.poll_input(&mut inputs).map_err(|source| {
                    HostError::Adapter {
                        adapter: adapter.config.grant.adapter_id.clone(),
                        source,
                    }
                })?
                else {
                    idle = true;
                    break;
                };
                adapter.report.inputs.push(input.clone());
                received.push(input);
            }
            if !idle {
                adapter.report.input_limit_hits += 1;
            }
        }
        Ok(received)
    }

    /// Im Dauerbetrieb enthält der Bericht nur Inputs der aktuellen Runde.
    pub(crate) fn poll_event_tick(&mut self) -> Result<(), HostError> {
        for adapter in &mut self.adapters {
            adapter.report.inputs.clear();
        }
        self.poll_inputs()?;
        self.poll_round()
    }
}
