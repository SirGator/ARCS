//! Nonblocking Request transport. Only the Host prepares or commits Core state.
use super::*;
use arcs::adapters::AdapterCallError;
use arcs::request::{PreparedRequest, RequestAdapter, RequestPreparation, RequestResponse};
use arcs::runtime::{InvocationService, InvocationStatus};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

pub(super) struct PendingRequest {
    prepared: PreparedRequest,
    response: Receiver<Result<RequestResponse, AdapterCallError>>,
    worker: JoinHandle<()>,
    deadline: Instant,
    discarded: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct RequestWorkerReport {
    pub invocation_id: String,
    pub capability: CapabilityRef,
    pub input_version_id: VersionId,
    pub discarded: bool,
    pub retiring: bool,
}

pub(super) struct RetiringRequest {
    pub(super) report: RequestWorkerReport,
    worker: JoinHandle<()>,
}

fn reap_retiring(workers: &mut Vec<RetiringRequest>) {
    for worker in std::mem::take(workers) {
        if worker.worker.is_finished() {
            let _ = worker.worker.join();
        } else {
            workers.push(worker);
        }
    }
}
impl PendingRequest {
    pub(super) fn adapter_id(&self) -> &AdapterId {
        &self.prepared.invocation().capability.adapter_id
    }
}

impl Host {
    pub fn request_workers(&self) -> Vec<RequestWorkerReport> {
        let mut jobs: Vec<_> = self
            .request_jobs
            .values()
            .map(|job| {
                let inv = job.prepared.invocation();
                RequestWorkerReport {
                    invocation_id: inv.invocation_id.clone(),
                    capability: inv.capability.clone(),
                    input_version_id: inv.request_version_id.clone(),
                    discarded: job.discarded,
                    retiring: false,
                }
            })
            .collect();
        jobs.extend(self.retiring_request_jobs.iter().map(|j| j.report.clone()));
        jobs.sort_by(|a, b| a.invocation_id.cmp(&b.invocation_id));
        jobs
    }

    fn request_service(&mut self) -> RequestService<'_> {
        RequestService::new(
            &self.registry,
            &self.schemas,
            &self.store,
            &mut self.ids,
            &SystemClock,
        )
    }

    /// Bounded across all runtimes sharing this Host. None means queued/running.
    /// A completed replay uses no worker and never contacts a busy adapter.
    pub fn poll_request_version(
        &mut self,
        capability: &CapabilityRef,
        input: &VersionId,
        response_schema: &SchemaId,
        limit: usize,
    ) -> Result<Option<Artifact>, HostError> {
        if !(1..=32).contains(&limit) {
            return Err(invalid("request worker limit must be 1..32"));
        }
        self.reap_discarded_requests();
        let key = arcs::request::request_invocation_id(capability, input, response_schema);
        if self.request_jobs.contains_key(&key) {
            return self.collect_request(&key);
        }
        let cached = InvocationService::new(&self.store, &self.schemas, &SystemClock)
            .lookup(&key)
            .map_err(|source| request_error(input, source.into()))?
            .is_some_and(|state| state.status == InvocationStatus::Succeeded);
        if !cached {
            if self.request_jobs.len() + self.retiring_request_jobs.len() >= limit {
                return Ok(None);
            }
            match self.ensure_adapter_available(&capability.adapter_id) {
                Err(HostError::AdapterBusy(_)) => return Ok(None),
                other => other?,
            }
        }
        let adapter = self
            .adapters
            .iter()
            .find(|a| a.config.grant.adapter_id == capability.adapter_id)
            .ok_or_else(|| invalid("unknown request adapter"))?;
        let endpoint = Arc::clone(&adapter.endpoint);
        let timeout = Duration::from_millis(adapter.config.timeout_ms);
        let prepared = match self
            .request_service()
            .prepare(endpoint.as_ref(), capability, input, response_schema)
            .map_err(|e| request_error(input, e))?
        {
            RequestPreparation::Complete(result) => return Ok(Some(*result)),
            RequestPreparation::Dispatch(prepared) => *prepared,
        };
        let invocation = prepared.invocation().clone();
        let deadline = Instant::now() + timeout;
        let (send, response) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("arcs-request".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    endpoint.fetch(&invocation)
                }))
                .unwrap_or_else(|_| {
                    Err(AdapterCallError::Unavailable(
                        "request worker panicked".into(),
                    ))
                });
                let result = if Instant::now() >= deadline {
                    Err(AdapterCallError::Timeout)
                } else {
                    result
                };
                let _ = send.send(result);
            });
        match worker {
            Ok(worker) => {
                self.request_jobs.insert(
                    key,
                    PendingRequest {
                        prepared,
                        response,
                        worker,
                        deadline,
                        discarded: false,
                    },
                );
                Ok(None)
            }
            Err(error) => self
                .request_service()
                .finish(
                    prepared,
                    Err(AdapterCallError::Unavailable(format!(
                        "cannot start request worker: {error}"
                    ))),
                )
                .map(Some)
                .map_err(|e| request_error(input, e)),
        }
    }

    fn collect_request(&mut self, key: &str) -> Result<Option<Artifact>, HostError> {
        let Some(job) = self.request_jobs.get(key) else {
            return Ok(None);
        };
        if job.discarded {
            return Err(invalid("request was abandoned by its task"));
        }
        let response = match job.response.try_recv() {
            Ok(response) => response,
            Err(TryRecvError::Empty) if Instant::now() < job.deadline => return Ok(None),
            Err(TryRecvError::Empty) => Err(AdapterCallError::Timeout),
            Err(TryRecvError::Disconnected) => Err(AdapterCallError::Unavailable(
                "request worker disconnected".into(),
            )),
        };
        let job = self.request_jobs.remove(key).expect("checked request");
        if response.is_err() {
            self.abort_request_adapter(job.adapter_id());
        }
        let prepared = self.retain_request_worker(job);
        let input = prepared.invocation().request_version_id.clone();
        self.request_service()
            .finish(prepared, response)
            .map(Some)
            .map_err(|e| request_error(&input, e))
    }

    fn retain_request_worker(&mut self, job: PendingRequest) -> PreparedRequest {
        if job.worker.is_finished() {
            let _ = job.worker.join();
        } else {
            let inv = job.prepared.invocation();
            self.retiring_request_jobs.push(RetiringRequest {
                report: RequestWorkerReport {
                    invocation_id: inv.invocation_id.clone(),
                    capability: inv.capability.clone(),
                    input_version_id: inv.request_version_id.clone(),
                    discarded: job.discarded,
                    retiring: true,
                },
                worker: job.worker,
            });
        }
        job.prepared
    }

    fn abort_request_adapter(&mut self, adapter_id: &AdapterId) {
        if let Some(adapter) = self
            .adapters
            .iter()
            .find(|a| &a.config.grant.adapter_id == adapter_id)
        {
            adapter.endpoint.abort();
        }
        self.unavailable_adapters.insert(adapter_id.clone());
    }

    pub(crate) fn pending_request_inputs(&self) -> Vec<VersionId> {
        self.request_jobs
            .values()
            .map(|j| j.prepared.invocation().request_version_id.clone())
            .collect()
    }

    /// Collect existing work only, including while its task is paused.
    pub(crate) fn collect_request_input(
        &mut self,
        input: &VersionId,
    ) -> Result<Option<Artifact>, HostError> {
        let key = self
            .request_jobs
            .iter()
            .find(|(_, j)| &j.prepared.invocation().request_version_id == input)
            .map(|(key, _)| key.clone());
        match key {
            Some(key) => self.collect_request(&key),
            None => Ok(None),
        }
    }

    /// Call only after the owning task transition has committed.
    pub(crate) fn discard_request_inputs(&mut self, inputs: &HashSet<VersionId>) {
        for job in self.request_jobs.values_mut() {
            if inputs.contains(&job.prepared.invocation().request_version_id) {
                job.discarded = true;
            }
        }
        self.reap_discarded_requests();
    }

    fn reap_discarded_requests(&mut self) {
        reap_retiring(&mut self.retiring_request_jobs);
        let mut ready = Vec::new();
        for (key, job) in &self.request_jobs {
            if !job.discarded {
                continue;
            }
            let abort = match job.response.try_recv() {
                Ok(Ok(_)) => false,
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => true,
                Err(TryRecvError::Empty) if Instant::now() >= job.deadline => true,
                Err(TryRecvError::Empty) => continue,
            };
            ready.push((key.clone(), abort));
        }
        for (key, abort) in ready {
            let job = self.request_jobs.remove(&key).expect("discarded request");
            if abort {
                self.abort_request_adapter(job.adapter_id());
            }
            // Never fabricate a successful Core result for abandoned work.
            let _ = self.retain_request_worker(job);
        }
    }

    pub(super) fn cancel_request_jobs(&mut self) -> HashSet<AdapterId> {
        let adapters: HashSet<_> = self
            .request_jobs
            .values()
            .map(|j| j.adapter_id().clone())
            .chain(
                self.retiring_request_jobs
                    .iter()
                    .map(|j| j.report.capability.adapter_id.clone()),
            )
            .collect();
        for id in &adapters {
            self.abort_request_adapter(id);
        }
        for (_, job) in self.request_jobs.drain() {
            let _ = job.worker.join();
        }
        for job in self.retiring_request_jobs.drain(..) {
            let _ = job.worker.join();
        }
        adapters
    }
}

fn request_error(input: &VersionId, source: arcs::request::RequestError) -> HostError {
    HostError::Request {
        request: input.0.clone(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_retiring_worker_remains_accounted_until_it_has_actually_exited() {
        let (started, ready) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            started.send(()).unwrap();
            gate.recv().unwrap();
        });
        ready.recv().unwrap();
        let mut workers = vec![RetiringRequest {
            report: RequestWorkerReport {
                invocation_id: "test".into(),
                capability: CapabilityRef::new("test", "read"),
                input_version_id: VersionId("input".into()),
                discarded: false,
                retiring: true,
            },
            worker,
        }];
        reap_retiring(&mut workers);
        assert_eq!(workers.len(), 1);
        assert!(!workers[0].worker.is_finished());
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !workers.is_empty() {
            reap_retiring(&mut workers);
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
    }
}
