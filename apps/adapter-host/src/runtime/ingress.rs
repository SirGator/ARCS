//! Admit Input and Observe dialogs without waiting on the transport thread.
use std::collections::BTreeMap;

use arcs::adapters::stdio::{CommittedIngress, IngressKind, IngressReply, StdioError};
use arcs::input::InputError;
use arcs::store::StoreError;
use arcs::world::observation::ObservationError;

use super::*;

const MAX_DIALOGS: usize = 8;

pub(crate) struct IngressSelection {
    pub all_inputs: bool,
    pub wake_inputs: HashSet<AdapterId>,
    pub observations: bool,
}

impl IngressSelection {
    fn allows(&self, adapter: &AdapterId, kind: IngressKind) -> bool {
        match kind {
            IngressKind::Input => self.all_inputs || self.wake_inputs.contains(adapter),
            IngressKind::Observation => self.observations,
        }
    }
}

#[derive(Default)]
pub(super) struct IngressDialogs {
    active: BTreeMap<usize, IngressDialog>,
    cursor: usize,
}

#[derive(Clone, Copy)]
struct IngressDialog {
    kind: IngressKind,
    acknowledging: bool,
}

impl Host {
    pub(super) fn ingress_adapters(&self) -> impl Iterator<Item = AdapterId> + '_ {
        self.ingress
            .active
            .keys()
            .map(|index| self.adapters[*index].config.grant.adapter_id.clone())
    }

    pub(crate) fn collect_live_ingress(
        &mut self,
        selection: &IngressSelection,
    ) -> Result<bool, HostError> {
        let mut committed = false;
        for adapter in &mut self.adapters {
            adapter.report.inputs.clear();
        }
        let active: Vec<_> = self
            .ingress
            .active
            .iter()
            .map(|(&index, &dialog)| (index, dialog))
            .collect();
        for (index, dialog) in active {
            let reply = self.adapters[index].endpoint.collect_ingress();
            if matches!(reply, Ok(None)) {
                continue;
            }
            self.ingress.active.remove(&index);
            let result = match reply {
                Ok(Some(reply)) => self.accept_ingress(index, dialog, reply, selection),
                Err(error) => Err(HostError::Adapter {
                    adapter: self.adapters[index].config.grant.adapter_id.clone(),
                    source: error,
                }),
                Ok(None) => unreachable!("pending ingress dialog"),
            };
            match result {
                Ok(changed) => committed |= changed,
                Err(error) => self.fail_ingress(index, error)?,
            }
        }
        Ok(committed)
    }

    fn accept_ingress(
        &mut self,
        index: usize,
        dialog: IngressDialog,
        reply: IngressReply,
        selection: &IngressSelection,
    ) -> Result<bool, HostError> {
        let adapter = &mut self.adapters[index];
        let id = &adapter.config.grant.adapter_id;
        let adapter_error = |source| HostError::Adapter {
            adapter: id.clone(),
            source,
        };
        let IngressReply::Message(message) = reply else {
            return Ok(false);
        };
        // Capacity can change while the poll is in flight. The peer's
        // durable outbox will offer an input again until it sees its ACK.
        if dialog.kind == IngressKind::Input && !selection.allows(id, dialog.kind) {
            return Ok(false);
        }
        let (committed, ack) = message
            .commit(
                &self.registry,
                &self.schemas,
                &self.store,
                &mut self.ids,
                &SystemClock,
            )
            .map_err(adapter_error)?;
        match committed {
            CommittedIngress::Input(artifact) => adapter.report.inputs.push(artifact),
            CommittedIngress::Observation(recorded) => {
                Self::reduce_observation(&mut self.world, &mut adapter.report, &recorded)?;
            }
        }
        adapter
            .endpoint
            .start_ingress_ack(ack)
            .map_err(adapter_error)?;
        self.ingress.active.insert(
            index,
            IngressDialog {
                acknowledging: true,
                ..dialog
            },
        );
        Ok(true)
    }

    pub(crate) fn start_live_ingress(
        &mut self,
        selection: &IngressSelection,
    ) -> Result<(), HostError> {
        let mut busy = self.busy_adapters();
        let candidates = self.adapters.len() * 2;
        let cursor = self.ingress.cursor;
        for offset in 0..candidates {
            if self.ingress.active.len() >= MAX_DIALOGS {
                break;
            }
            let slot = (cursor + offset) % candidates;
            let index = slot / 2;
            let kind = [IngressKind::Input, IngressKind::Observation][slot % 2];
            let adapter = &self.adapters[index];
            let id = &adapter.config.grant.adapter_id;
            if !selection.allows(id, kind)
                || busy.contains(id)
                || self.unavailable_adapters.contains(id)
            {
                continue;
            }
            let enabled = adapter
                .endpoint
                .manifest()
                .capabilities
                .iter()
                .any(|capability| {
                    let role = match kind {
                        IngressKind::Input => capability.contract.is_input(),
                        IngressKind::Observation => capability.contract.is_observation(),
                    };
                    role && adapter
                        .config
                        .grant
                        .enabled_capabilities
                        .contains(&capability.id)
                });
            if !enabled {
                continue;
            }
            self.ingress.cursor = (slot + 1) % candidates;
            match adapter.endpoint.start_ingress_poll(kind) {
                Ok(()) => {
                    busy.insert(id.clone());
                    self.ingress.active.insert(
                        index,
                        IngressDialog {
                            kind,
                            acknowledging: false,
                        },
                    );
                }
                Err(source) => {
                    let error = HostError::Adapter {
                        adapter: id.clone(),
                        source,
                    };
                    self.fail_ingress(index, error)?;
                }
            }
        }
        Ok(())
    }

    fn fail_ingress(&mut self, index: usize, error: HostError) -> Result<(), HostError> {
        let adapter = &mut self.adapters[index];
        adapter.endpoint.stop_ingress();
        self.unavailable_adapters
            .insert(adapter.config.grant.adapter_id.clone());
        match error {
            HostError::Adapter { source, .. } => match source {
                StdioError::Input(InputError::Store(store))
                | StdioError::Observation(ObservationError::Store(store))
                    if !matches!(store, StoreError::IdempotencyConflict) =>
                {
                    Err(HostError::Store(store))
                }
                source => {
                    adapter.report.ingress_error =
                        Some(format!("{source:?}").chars().take(2000).collect());
                    Ok(())
                }
            },
            error => Err(error),
        }
    }

    pub(crate) fn ingress_status(&self) -> Vec<serde_json::Value> {
        self.ingress
            .active
            .iter()
            .map(|(&index, dialog)| {
                json!({
                    "adapter_id":self.adapters[index].config.grant.adapter_id,
                    "kind":dialog.kind,
                    "phase":if dialog.acknowledging { "acknowledging" } else { "polling" }
                })
            })
            .collect()
    }

    pub(super) fn stop_ingress(&mut self) -> HashSet<AdapterId> {
        std::mem::take(&mut self.ingress.active)
            .into_keys()
            .map(|index| {
                let adapter = &self.adapters[index];
                adapter.endpoint.stop_ingress();
                adapter.config.grant.adapter_id.clone()
            })
            .collect()
    }
}
