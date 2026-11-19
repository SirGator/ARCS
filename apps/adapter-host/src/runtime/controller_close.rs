//! Controller workers own transport only. A completed reply is committed in the Host.
use super::*;
use arcs::runtime::reservations::{
    ControllerAcknowledgement, ControllerClosure, ReservationService,
};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Instant;

const MAX_WORKERS: usize = 4;
const RETRY_DELAY: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(super) struct ControllerWorkers {
    active: HashMap<AdapterId, ClosureWorker>,
    failures: HashMap<AdapterId, ClosureFailure>,
}

struct ClosureWorker {
    closure: ControllerClosure,
    endpoint: Arc<StdioAdapter>,
    response: Receiver<Result<ControllerAcknowledgement, AdapterCallError>>,
    thread: JoinHandle<()>,
}

struct ClosureFailure {
    closure: ControllerClosure,
    source: AdapterCallError,
    retry_at: Instant,
}

impl Host {
    pub fn controller_closures(&self) -> Vec<ControllerClosure> {
        let workers = self.controller_workers.borrow();
        let mut active: Vec<_> = workers
            .active
            .values()
            .map(|worker| worker.closure.clone())
            .collect();
        active.sort_by(|a, b| a.capability().cmp(b.capability()));
        active
    }

    pub(super) fn controller_worker_adapters(&self) -> Vec<AdapterId> {
        self.controller_workers
            .borrow()
            .active
            .keys()
            .cloned()
            .collect()
    }

    pub(crate) fn start_controller_closure(
        &self,
        closure: &ControllerClosure,
    ) -> Result<(), ReservationError> {
        let capability = closure.capability();
        let adapter = &capability.adapter_id;
        let failure = |source| ReservationError::ControllerAcknowledgement {
            capability: capability.clone(),
            source,
        };
        {
            let workers = self.controller_workers.borrow();
            if let Some(previous) = workers.failures.get(adapter).filter(|previous| {
                &previous.closure == closure && Instant::now() < previous.retry_at
            }) {
                return Err(failure(previous.source.clone()));
            }
            if workers.active.len() >= MAX_WORKERS || workers.active.contains_key(adapter) {
                return Ok(());
            }
        }
        if self.busy_adapters().contains(adapter) {
            return Ok(());
        }
        let endpoint = self.reservation_controller_endpoint(capability)?.clone();
        let request = closure.clone();
        let worker_endpoint = endpoint.clone();
        let (send, response) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("arcs-close-fence".into())
            .spawn(move || {
                let _ = send.send(request.confirm(worker_endpoint.as_ref()));
            })
            .map_err(|error| {
                failure(AdapterCallError::Unavailable(format!(
                    "cannot start controller worker: {error}"
                )))
            })?;
        self.controller_workers.borrow_mut().active.insert(
            adapter.clone(),
            ClosureWorker {
                closure: closure.clone(),
                endpoint,
                response,
                thread,
            },
        );
        Ok(())
    }

    pub(crate) fn collect_controller_closures(&mut self) -> Result<(), HostError> {
        let workers = self.controller_workers.get_mut();
        let finished: Vec<_> = workers
            .active
            .iter()
            .filter(|(_, worker)| worker.thread.is_finished())
            .map(|(adapter, _)| adapter.clone())
            .collect();
        let service = ReservationService::new(&self.store, &self.schemas, &SystemClock);
        for adapter in finished {
            let worker = workers.active.remove(&adapter).expect("finished worker");
            let _ = worker.thread.join();
            match worker.response.try_recv().unwrap_or_else(|_| {
                Err(AdapterCallError::Unavailable(
                    "controller worker disconnected".into(),
                ))
            }) {
                Ok(acknowledgement) => {
                    service
                        .record_controller_acknowledgement(acknowledgement)
                        .map_err(|failure| match failure {
                            ReservationError::Store(error) => HostError::Store(error),
                            other => {
                                invalid(format!("invalid controller acknowledgement: {other:?}"))
                            }
                        })?;
                    workers.failures.remove(&adapter);
                }
                Err(source) => {
                    if !matches!(source, AdapterCallError::Rejected(_)) {
                        worker.endpoint.abort();
                        self.unavailable_adapters.insert(adapter.clone());
                    }
                    workers.failures.insert(
                        adapter,
                        ClosureFailure {
                            closure: worker.closure,
                            source,
                            retry_at: Instant::now() + RETRY_DELAY,
                        },
                    );
                }
            }
        }
        Ok(())
    }

    pub(super) fn cancel_controller_closures(&mut self) -> HashSet<AdapterId> {
        let workers = std::mem::take(&mut self.controller_workers.get_mut().active);
        for worker in workers.values() {
            worker.endpoint.abort();
        }
        workers
            .into_iter()
            .map(|(adapter, worker)| {
                let _ = worker.thread.join();
                adapter
            })
            .collect()
    }
}
