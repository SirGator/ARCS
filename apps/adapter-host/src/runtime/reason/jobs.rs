//! Begrenzte externe Reasoning-Arbeit. Nur der Host schreibt in den Core-Store.
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use arcs::adapters::AdapterCallError;
use arcs::adapters::input_data::{InputDataSelection, InputDataService};
use arcs::reasoning::general::{
    GeneralReasoningAdapter, GeneralReasoningService, PreparedReasoning, ReasoningPreparation,
    ReasoningResult,
};
use arcs::reasoning::{ReasoningError, ReasoningRequestV2, ReasoningResponseV2};
use arcs::runtime::InvocationStatus;

use super::*;

pub(in crate::runtime) struct PendingReasoning {
    pub(super) prepared: PreparedReasoning,
    fingerprint: String,
    pub(in crate::runtime) resources: HashSet<AdapterId>,
    pub(super) response: Receiver<Result<ReasoningResponseV2, ReasoningError>>,
    pub(super) worker: JoinHandle<()>,
    pub(super) deadline: Instant,
    pub(super) discarded: bool,
    pub(in crate::runtime) claims: crate::runtime::resources::ResourceClaims,
}

impl Host {
    fn reasoning_service(&mut self) -> GeneralReasoningService<'_> {
        GeneralReasoningService {
            registry: &self.registry,
            schemas: &self.schemas,
            store: &self.store,
            ids: &mut self.ids,
            clock: &SystemClock,
        }
    }

    /// None bedeutet laufend oder wartend auf Kapazität. Der gleiche Auftrag
    /// wird anhand seiner stabilen Identität abgeholt, niemals doppelt gestartet.
    pub(crate) fn poll_reason_with_data(
        &mut self,
        request: ReasoningRequestV2,
        inputs: &[InputDataSelection],
        limit: usize,
    ) -> Result<Option<ReasoningResult>, HostError> {
        self.reap_discarded_reasoning();
        let key = Self::reasoning_key(&request);
        let fingerprint = serde_json::to_string(&(&request, inputs))
            .map_err(|error| invalid(error.to_string()))?;
        if let Some(job) = self.reasoning_jobs.get(&key) {
            if job.discarded {
                return Err(invalid("reasoning request was cancelled"));
            }
            if job.fingerprint != fingerprint {
                return Err(invalid("running reasoning request identity changed"));
            }
            return self.collect_pending_reasoning(&key);
        }
        let recorded = matches!(
            self.reasoning_status(&key)?,
            Some(InvocationStatus::Succeeded | InvocationStatus::Failed)
        );
        if !recorded && self.reasoning_jobs.len() >= limit {
            return Ok(None);
        }
        let resources: HashSet<_> = std::iter::once(request.capability.adapter_id.clone())
            .chain(inputs.iter().map(|input| input.source.adapter_id.clone()))
            .collect();
        for resource in resources.iter().filter(|_| !recorded) {
            match self.ensure_adapter_available(resource) {
                Err(HostError::AdapterBusy(_)) => return Ok(None),
                other => other?,
            }
        }
        let claims = if recorded {
            crate::runtime::resources::ResourceClaims::new()
        } else {
            self.reasoning_claims(&resources)?
        };
        if !recorded {
            self.ensure_resources_available(&claims)?;
        }
        let endpoint = self
            .adapters
            .iter()
            .find(|adapter| adapter.config.grant.adapter_id == request.capability.adapter_id)
            .map(|adapter| Arc::clone(&adapter.endpoint))
            .ok_or_else(|| invalid("unknown reasoning adapter"))?;
        let prepared = match self
            .reasoning_service()
            .prepare_for_worker(endpoint.as_ref(), request, inputs)
            .map_err(HostError::Reasoning)?
        {
            ReasoningPreparation::Complete(result) => return Ok(Some(result)),
            ReasoningPreparation::Dispatch(prepared) => *prepared,
        };
        if recorded {
            return Err(invalid("recorded reasoning state changed during admission"));
        }
        let invocation = prepared.invocation().clone();
        let deadline = Instant::now() + Duration::from_millis(invocation.budget.max_runtime_ms);
        let registry = self.registry.clone();
        let sources: Vec<_> = self
            .adapters
            .iter()
            .filter(|adapter| resources.contains(&adapter.config.grant.adapter_id))
            .map(|adapter| Arc::clone(&adapter.endpoint))
            .collect();
        let (send, response) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("arcs-reasoning".into())
            .spawn(move || {
                let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let started = Instant::now();
                    let sources: Vec<_> = sources
                        .iter()
                        .map(|source| source.as_ref() as &dyn arcs::adapters::data::DataAdapter)
                        .collect();
                    let inputs = InputDataService::read_prepared(
                        &registry,
                        &invocation.input_data,
                        &sources,
                    )?;
                    if started.elapsed() >= Duration::from_millis(invocation.budget.max_runtime_ms)
                    {
                        return Err(ReasoningError::AdapterCall(AdapterCallError::Timeout));
                    }
                    let budget = invocation.budget.max_runtime_ms;
                    let result = endpoint.propose_with_data(&invocation, &inputs)?;
                    if started.elapsed() > Duration::from_millis(budget) {
                        return Err(ReasoningError::AdapterCall(AdapterCallError::Timeout));
                    }
                    Ok(result)
                }))
                .unwrap_or_else(|_| {
                    Err(ReasoningError::AdapterCall(AdapterCallError::Unavailable(
                        "reasoning worker panicked".into(),
                    )))
                });
                let _ = send.send(response);
            });
        match worker {
            Ok(worker) => {
                self.reasoning_jobs.insert(
                    key,
                    PendingReasoning {
                        prepared,
                        fingerprint,
                        resources,
                        response,
                        worker,
                        deadline,
                        discarded: false,
                        claims,
                    },
                );
                Ok(None)
            }
            Err(error) => self
                .reasoning_service()
                .finish(
                    prepared,
                    Err(ReasoningError::AdapterCall(AdapterCallError::Unavailable(
                        format!("cannot start reasoning worker: {error}"),
                    ))),
                )
                .map(Some)
                .map_err(HostError::Reasoning),
        }
    }

    /// Collect only an existing worker. This entry point never prepares or
    /// dispatches a new request, including after a host restart.
    fn collect_pending_reasoning(
        &mut self,
        key: &str,
    ) -> Result<Option<ReasoningResult>, HostError> {
        let Some(job) = self.reasoning_jobs.get(key) else {
            return Ok(None);
        };
        let (response, aborted) = match job.response.try_recv() {
            Ok(response) => (response, false),
            Err(TryRecvError::Empty) if Instant::now() < job.deadline => return Ok(None),
            Err(TryRecvError::Empty) => {
                for adapter in &self.adapters {
                    if job.resources.contains(&adapter.config.grant.adapter_id) {
                        adapter.endpoint.abort();
                    }
                }
                (
                    Err(ReasoningError::AdapterCall(AdapterCallError::Timeout)),
                    true,
                )
            }
            Err(TryRecvError::Disconnected) => (
                Err(ReasoningError::AdapterCall(AdapterCallError::Unavailable(
                    "reasoning worker disconnected".into(),
                ))),
                false,
            ),
        };
        let job = self
            .reasoning_jobs
            .remove(key)
            .expect("checked running job");
        if let Err(error) = &response {
            self.quarantine_reasoning_failure(
                &job.prepared.invocation().capability.adapter_id,
                &job.resources,
                error,
            );
        }
        // An elapsed overall deadline aborts every session reserved for
        // this call; none may be reused while a late worker unwinds.
        if aborted {
            self.unavailable_adapters
                .extend(job.resources.iter().cloned());
        }
        if job.worker.is_finished() {
            let _ = job.worker.join();
        }
        self.reasoning_service()
            .finish(job.prepared, response)
            .map(Some)
            .map_err(HostError::Reasoning)
    }

    pub(crate) fn collect_reasoning_requests(
        &mut self,
        requests: &HashSet<String>,
    ) -> Vec<(String, Result<Option<ReasoningResult>, HostError>)> {
        let pending: Vec<_> = self
            .reasoning_jobs
            .iter()
            .filter(|(_, job)| {
                !job.discarded && requests.contains(&job.prepared.invocation().request_id)
            })
            .map(|(key, job)| (key.clone(), job.prepared.invocation().request_id.clone()))
            .collect();
        pending
            .into_iter()
            .map(|(key, id)| (id, self.collect_pending_reasoning(&key)))
            .collect()
    }

    pub(in crate::runtime) fn cancel_reasoning_jobs(&mut self) -> HashSet<AdapterId> {
        let resources = self
            .reasoning_jobs
            .values()
            .flat_map(|job| job.resources.iter().cloned())
            .collect::<HashSet<_>>();
        for adapter in &self.adapters {
            if resources.contains(&adapter.config.grant.adapter_id) {
                adapter.endpoint.abort();
            }
        }
        // Workers never write Core state. Uncommitted dispatches retain their
        // unknown outcome and can only be recovered using the same invocation.
        for (_, job) in self.reasoning_jobs.drain() {
            let _ = job.worker.join();
        }
        self.unavailable_adapters.extend(resources.iter().cloned());
        resources
    }
}

impl Host {
    /// A known quarantined port rejects a new explicitly authorized recovery
    /// request locally. This records an admission failure, without contacting
    /// the old session, reading binary sources, or fabricating backend work.
    pub(crate) fn reject_quarantined_recovery(
        &mut self,
        request: ReasoningRequestV2,
        inputs: &[InputDataSelection],
    ) -> Result<ReasoningResult, HostError> {
        if request
            .constraints
            .get(arcs::reasoning::recovery::CONSTRAINT)
            .is_none()
            || !self
                .unavailable_adapters
                .contains(&request.capability.adapter_id)
        {
            return Err(invalid(
                "local recovery rejection requires a quarantined recovery endpoint",
            ));
        }
        let endpoint = self
            .adapters
            .iter()
            .find(|adapter| adapter.config.grant.adapter_id == request.capability.adapter_id)
            .map(|adapter| Arc::clone(&adapter.endpoint))
            .ok_or_else(|| invalid("unknown recovery endpoint"))?;
        match self.reasoning_service().prepare_for_worker(endpoint.as_ref(),request,inputs).map_err(HostError::Reasoning)? {
            ReasoningPreparation::Complete(result)=>Ok(result),
            ReasoningPreparation::Dispatch(prepared)=>self.reasoning_service().finish(*prepared,Err(ReasoningError::AdapterCall(AdapterCallError::Unavailable("reasoning session is quarantined; request rejected locally before backend delivery".into())))).map_err(HostError::Reasoning),
        }
    }
}
