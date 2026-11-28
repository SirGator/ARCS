//! Act workers send an authorized call. The Host validates and commits the reply.
use super::*;
use arcs::adapters::ActResponse;
use arcs::execution::{ExecutionError, ExecutionPreparation, PreparedExecution};
use arcs::runtime::reservations::Reservation;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

const MAX_WORKERS: usize = 4;

#[derive(Clone, Serialize)]
pub struct ExecutionWorkerReport {
    pub sequence: u64,
    pub invocation_id: String,
    pub action_version: VersionId,
    pub capability: CapabilityRef,
}

pub(super) struct ExecutionWorker {
    journal: PathBuf,
    report: ExecutionWorkerReport,
    dispatch: PreparedExecution,
    endpoint: Arc<StdioAdapter>,
    response: Receiver<Result<ActResponse, AdapterCallError>>,
    thread: JoinHandle<()>,
}

impl Host {
    pub fn execution_workers(&self) -> Vec<&ExecutionWorkerReport> {
        let mut active: Vec<_> = self
            .act_workers
            .values()
            .map(|worker| &worker.report)
            .collect();
        active.sort_by(|a, b| a.capability.cmp(&b.capability));
        active
    }

    /// A running task owns its result. Only its own journal can collect it.
    pub(crate) fn owns_execution(&self, journal: &Path, sequence: u64, action: &VersionId) -> bool {
        self.act_workers.values().any(|worker| {
            worker.journal == journal
                && worker.report.sequence == sequence
                && worker.report.action_version == *action
        })
    }

    pub(crate) fn execution_slot_available(&self, adapter: &AdapterId) -> Result<bool, HostError> {
        match self.ensure_adapter_available(adapter) {
            Ok(()) => Ok(self.act_workers.len() < MAX_WORKERS),
            Err(HostError::AdapterBusy(_)) => Ok(false),
            Err(HostError::Adapter { source, .. }) => Err(HostError::Execution {
                adapter: adapter.clone(),
                source: ExecutionError::AdapterCall(AdapterCallError::Unavailable(format!(
                    "{source:?}"
                ))),
            }),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn start_execution(
        &mut self,
        journal: &Path,
        sequence: u64,
        adapter: &AdapterId,
        action: &VersionId,
        task_key: &str,
        reservation: Option<&Reservation>,
    ) -> Result<Option<Artifact>, HostError> {
        if !self.execution_slot_available(adapter)? {
            return Err(HostError::AdapterBusy(adapter.clone()));
        }
        let endpoint = self
            .adapters
            .iter()
            .find(|a| a.config.grant.adapter_id == *adapter)
            .ok_or_else(|| invalid(format!("unknown adapter {}", adapter.0)))?
            .endpoint
            .clone();
        let failure = |source| HostError::Execution {
            adapter: adapter.clone(),
            source,
        };
        let mut service = self.execution_service();
        let prepared = match service
            .prepare(endpoint.as_ref(), action, task_key, reservation)
            .map_err(failure)?
        {
            ExecutionPreparation::Complete(result) => return Ok(Some(*result)),
            ExecutionPreparation::Dispatch(prepared) => *prepared,
        };

        let call = prepared.call().clone();
        let invocation = call.invocation();
        let report = ExecutionWorkerReport {
            sequence,
            invocation_id: invocation.invocation_id.clone(),
            action_version: action.clone(),
            capability: invocation.capability.clone(),
        };
        let worker_endpoint = endpoint.clone();
        let (send, response) = mpsc::sync_channel(1);
        let thread = match thread::Builder::new()
            .name("arcs-act".into())
            .spawn(move || {
                let _ = send.send(call.send(worker_endpoint.as_ref()));
            }) {
            Ok(thread) => thread,
            Err(error) => {
                return service
                    .finish(
                        prepared,
                        Err(AdapterCallError::Unavailable(format!(
                            "cannot start Act worker: {error}"
                        ))),
                    )
                    .map(Some)
                    .map_err(failure);
            }
        };
        self.act_workers.insert(
            adapter.clone(),
            ExecutionWorker {
                journal: journal.into(),
                report,
                dispatch: prepared,
                endpoint,
                response,
                thread,
            },
        );
        Ok(None)
    }

    pub(crate) fn finished_executions(&self, journal: &Path) -> Vec<ExecutionWorkerReport> {
        self.act_workers
            .values()
            .filter(|worker| worker.journal == journal && worker.thread.is_finished())
            .map(|worker| worker.report.clone())
            .collect()
    }

    pub(crate) fn collect_execution(
        &mut self,
        adapter: &AdapterId,
    ) -> Result<Option<Artifact>, HostError> {
        if !self
            .act_workers
            .get(adapter)
            .is_some_and(|worker| worker.thread.is_finished())
        {
            return Ok(None);
        }
        let worker = self
            .act_workers
            .remove(adapter)
            .expect("finished Act worker");
        let _ = worker.thread.join();
        let response = worker.response.try_recv().unwrap_or_else(|_| {
            Err(AdapterCallError::Unavailable(
                "Act worker disconnected".into(),
            ))
        });
        if response.is_err() {
            worker.endpoint.abort();
            self.unavailable_adapters.insert(adapter.clone());
        }
        self.execution_service()
            .finish(worker.dispatch, response)
            .map(Some)
            .map_err(|source| HostError::Execution {
                adapter: adapter.clone(),
                source,
            })
    }

    pub(super) fn cancel_execution_workers(&mut self) -> HashSet<AdapterId> {
        let workers = std::mem::take(&mut self.act_workers);
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

    pub(super) fn execution_service(&mut self) -> ExecutionService<'_> {
        ExecutionService::new(
            &self.registry,
            &self.schemas,
            &self.store,
            &mut self.ids,
            &SystemClock,
        )
    }
}
