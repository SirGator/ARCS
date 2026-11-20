//! Output carries a Core-authorized invocation to an adapter and returns its reply.
use super::*;
use arcs::output::{
    OutputAdapter, OutputError, OutputInvocation, OutputPreparation, OutputResponse, OutputService,
    PreparedOutput,
};
use arcs::runtime::{
    InvocationKind, InvocationService, InvocationStatus, deterministic_invocation_id,
};
use std::fs::{File, TryLockError};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

const MAX_WORKERS: usize = 4;

pub(super) struct OutputWorker {
    dispatch: PreparedOutput,
    response: Receiver<Result<OutputResponse, AdapterCallError>>,
    thread: JoinHandle<()>,
    _session_lock: File,
}

impl Host {
    pub(crate) fn prepare_task_report(
        &mut self,
        adapter: &AdapterId,
        runtime_id: &str,
        sequence: u64,
        result: &VersionId,
    ) -> Result<Artifact, HostError> {
        self.output_service()
            .prepare_task_report(runtime_id, sequence, result)
            .map_err(|source| output_error(adapter, source))
    }

    pub(super) fn output_service(&mut self) -> OutputService<'_> {
        OutputService::new(
            &self.registry,
            &self.schemas,
            &self.store,
            &mut self.ids,
            &SystemClock,
        )
    }

    pub(super) fn output_endpoint(
        &self,
        adapter: &AdapterId,
    ) -> Result<Arc<StdioAdapter>, HostError> {
        self.adapters
            .iter()
            .find(|entry| entry.config.grant.adapter_id == *adapter)
            .map(|entry| entry.endpoint.clone())
            .ok_or_else(|| invalid(format!("unknown output adapter {}", adapter.0)))
    }

    /// Serialize receipt recovery for this adapter across hosts on the same
    /// Core. The OS releases the lock if the sending process is interrupted.
    pub(super) fn lock_output_session(&self, adapter: &AdapterId) -> Result<File, HostError> {
        let key =
            deterministic_invocation_id(InvocationKind::Output, &["stdio-session", &adapter.0]);
        let mut path = self.database_path.as_os_str().to_os_string();
        path.push(format!(".output-{key}.lock"));
        let path = PathBuf::from(path);
        let file = File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| HostError::Io {
                path: path.clone(),
                source,
            })?;
        match file.try_lock() {
            Ok(()) => Ok(file),
            Err(TryLockError::WouldBlock) => Err(HostError::AdapterBusy(adapter.clone())),
            Err(TryLockError::Error(source)) => Err(HostError::Io { path, source }),
        }
    }

    pub(crate) fn check_output_delivery(
        &mut self,
        adapter: &AdapterId,
        request: &VersionId,
        approval: &VersionId,
    ) -> Result<OutputInvocation, HostError> {
        let endpoint = self.output_endpoint(adapter)?;
        self.output_service()
            .check_delivery(
                OutputAdapter::manifest(endpoint.as_ref()),
                request,
                approval,
            )
            .map_err(|source| output_error(adapter, source))
    }

    pub(super) fn output_worker_adapters(&self) -> impl Iterator<Item = &AdapterId> {
        self.output_workers
            .values()
            .map(|worker| &worker.dispatch.invocation().capability.adapter_id)
    }

    pub(crate) fn output_worker_ids(&self) -> Vec<&str> {
        let mut ids: Vec<_> = self.output_workers.keys().map(String::as_str).collect();
        ids.sort_unstable();
        ids
    }

    pub(crate) fn poll_output_delivery(
        &mut self,
        adapter: &AdapterId,
        request: &VersionId,
        approval: &VersionId,
        id: &str,
    ) -> Result<Option<Artifact>, HostError> {
        if let Some(worker) = self.output_workers.get(id) {
            let call = worker.dispatch.invocation();
            if call.capability.adapter_id != *adapter
                || call.request_version_id != *request
                || call.approval_version_id != *approval
            {
                return Err(output_error(adapter, OutputError::StaleDispatch));
            }
            if !worker.thread.is_finished() {
                return Ok(None);
            }
            let worker = self
                .output_workers
                .remove(id)
                .expect("checked Output worker");
            let _ = worker.thread.join();
            let response = worker.response.try_recv().unwrap_or_else(|_| {
                Err(AdapterCallError::Unavailable(
                    "Output worker disconnected".into(),
                ))
            });
            if matches!(response, Err(ref error) if !matches!(error, AdapterCallError::Rejected(_)))
            {
                self.output_endpoint(adapter)?.abort();
                self.unavailable_adapters.insert(adapter.clone());
            }
            return self
                .output_service()
                .finish_delivery(worker.dispatch, response)
                .map(Some)
                .map_err(|source| output_error(adapter, source));
        }

        let endpoint = self.output_endpoint(adapter)?;
        let session_lock = self.lock_output_session(adapter)?;
        let recorded = InvocationService::new(&self.store, &self.schemas, &SystemClock)
            .lookup(id)
            .map_err(|source| output_error(adapter, source.into()))?
            .is_some_and(|state| {
                matches!(
                    state.status,
                    InvocationStatus::Succeeded | InvocationStatus::Failed
                )
            });
        if !recorded {
            if self.output_workers.len() >= MAX_WORKERS {
                return Ok(None);
            }
            self.ensure_adapter_available(adapter)?;
        }
        let manifest = OutputAdapter::manifest(endpoint.as_ref());
        let mut service = self.output_service();
        let dispatch = match service
            .prepare_delivery(manifest, request, approval, id)
            .map_err(|source| output_error(adapter, source))?
        {
            OutputPreparation::Complete(receipt) => return Ok(Some(*receipt)),
            OutputPreparation::Dispatch(dispatch) => *dispatch,
        };
        if recorded {
            return Err(output_error(adapter, OutputError::StaleDispatch));
        }
        let call = dispatch.invocation().clone();
        let (send, response) = mpsc::sync_channel(1);
        let thread = match thread::Builder::new()
            .name("arcs-output".into())
            .spawn(move || {
                let _ = send.send(endpoint.deliver(&call));
            }) {
            Ok(thread) => thread,
            Err(error) => {
                return service
                    .finish_delivery(
                        dispatch,
                        Err(AdapterCallError::Unavailable(format!(
                            "cannot start Output worker: {error}"
                        ))),
                    )
                    .map(Some)
                    .map_err(|source| output_error(adapter, source));
            }
        };
        self.output_workers.insert(
            id.into(),
            OutputWorker {
                dispatch,
                response,
                thread,
                _session_lock: session_lock,
            },
        );
        Ok(None)
    }

    pub(super) fn cancel_output_workers(&mut self) -> HashSet<AdapterId> {
        let active: HashSet<_> = self.output_worker_adapters().cloned().collect();
        for adapter in &self.adapters {
            if active.contains(&adapter.config.grant.adapter_id) {
                adapter.endpoint.abort();
            }
        }
        // An uncommitted receipt can be recovered from the peer under the same ID.
        for (_, worker) in self.output_workers.drain() {
            let _ = worker.thread.join();
        }
        self.unavailable_adapters.extend(active.iter().cloned());
        active
    }
}

fn output_error(adapter: &AdapterId, source: OutputError) -> HostError {
    HostError::Output {
        adapter: adapter.clone(),
        source,
    }
}
