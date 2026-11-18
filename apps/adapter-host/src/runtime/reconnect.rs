//! Background transport handshakes, followed by Core-controlled installation.
use super::*;
use arcs::adapters::reconnect::{
    ConnectionFailure, ConnectionState, ReconnectError, ReconnectReceipt, ReconnectService,
};
use arcs::adapters::stdio::StdioError;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) const MAX_WORKERS: usize = 2;
pub(super) struct ReconnectJob {
    claim: ReconnectReceipt,
    cancelled: Arc<AtomicBool>,
    response: Receiver<Result<StdioAdapter, ConnectionFailure>>,
    worker: Option<JoinHandle<()>>,
    result: Option<Result<StdioAdapter, ConnectionFailure>>,
    installed: bool,
    pub(super) claims: resources::ResourceClaims,
}
impl Drop for ReconnectJob {
    fn drop(&mut self) {
        if !self.installed {
            self.cancelled.store(true, Ordering::Release);
            if let Some(Ok(endpoint)) = &self.result {
                endpoint.abort();
            }
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
pub(super) fn error(error: ReconnectError) -> HostError {
    match error {
        ReconnectError::Store(error) => HostError::Store(error),
        ReconnectError::Conflict => invalid("reconnect state revision changed"),
        ReconnectError::Invalid(message) => invalid(message),
    }
}
pub(super) fn lock_node(database: &Path, node: &str) -> Result<std::fs::File, HostError> {
    let key = arcs::runtime::deterministic_invocation_id(
        arcs::runtime::InvocationKind::Reasoning,
        &["reconnect-node", node],
    );
    let path = std::path::PathBuf::from(format!("{}.reconnect-{key}.lock", database.display()));
    let file = std::fs::File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|source| HostError::Io {
            path: path.clone(),
            source,
        })?;
    file.try_lock().map_err(|_| {
        invalid("a live host already owns automatic reconnect for this Core and node_id")
    })?;
    Ok(file)
}

fn now() -> Result<u64, HostError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|n| n.as_millis().min(i64::MAX as u128) as u64)
        .map_err(|_| invalid("system time precedes Unix epoch"))
}
#[derive(Debug, Serialize)]
pub struct AdapterReconnectReport {
    pub adapter_id: AdapterId,
    pub connection: ReconnectReceipt,
    pub quarantined: bool,
    pub handshake_in_progress: bool,
}
impl Host {
    fn reconnect_service(&mut self) -> ReconnectService<'_> {
        ReconnectService {
            registry: &self.registry,
            schemas: &self.schemas,
            store: &self.store,
            ids: &mut self.ids,
            clock: &SystemClock,
        }
    }
    pub(super) fn reconnect_can_wait(&self, adapter: &AdapterId) -> bool {
        !self.reconnect_stopped
            && matches!(
                self.reconnect_states.get(adapter),
                Some(
                    ConnectionState::Healthy
                        | ConnectionState::Waiting
                        | ConnectionState::Connecting
                )
            )
    }
    pub fn adapter_reconnections(&self) -> Result<Vec<AdapterReconnectReport>, HostError> {
        if self.reconnect_installations.is_empty() {
            return Ok(vec![]);
        }
        let mut ids = self
            .store
            .durable_id_generator("reconnect-report")
            .map_err(HostError::Store)?;
        let service = ReconnectService {
            registry: &self.registry,
            schemas: &self.schemas,
            store: &self.store,
            ids: &mut ids,
            clock: &SystemClock,
        };
        self.reconnect_installations
            .iter()
            .map(|(adapter_id, definition)| {
                Ok(AdapterReconnectReport {
                    adapter_id: adapter_id.clone(),
                    quarantined: self.unavailable_adapters.contains(adapter_id),
                    handshake_in_progress: self.reconnect_jobs.contains_key(adapter_id),
                    connection: service.current(definition).map_err(error)?,
                })
            })
            .collect()
    }
    /// Drive maintenance when embedding Host without TaskRuntime. It never
    /// dispatches a model request, data query, action, transform or output.
    pub fn poll_reconnections(&mut self) -> Result<(), HostError> {
        self.poll_reconnections_at(now()?)
    }
    pub(crate) fn poll_reconnections_at(&mut self, now: u64) -> Result<(), HostError> {
        if self.reconnect_installations.is_empty() || self.reconnect_stopped {
            return Ok(());
        }
        if now > i64::MAX as u64 {
            return Err(invalid("reconnect timestamp exceeds SQLite range"));
        }
        self.collect_reconnections(now)?;
        let mut adapters = self
            .reconnect_installations
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        if let Some(cursor) = &self.reconnect_cursor {
            let split = adapters.partition_point(|id| id <= cursor);
            adapters.rotate_left(split);
        }
        for adapter_id in adapters {
            if self.reconnect_jobs.len() >= MAX_WORKERS {
                break;
            }
            if !self.unavailable_adapters.contains(&adapter_id)
                || self.reconnect_jobs.contains_key(&adapter_id)
            {
                continue;
            }
            let definition_version = self.reconnect_installations[&adapter_id].clone();
            let state = self
                .reconnect_service()
                .note_loss(&definition_version, now)
                .map_err(error)?;
            self.reconnect_states
                .insert(adapter_id.clone(), state.record.state);
            if state.record.state != ConnectionState::Waiting
                || state.record.not_before_unix_ms.is_none_or(|due| now < due)
            {
                continue;
            }
            let index = self
                .adapters
                .iter()
                .position(|adapter| adapter.config.grant.adapter_id == adapter_id)
                .ok_or_else(|| invalid("missing reconnect adapter"))?;
            // A returned timeout does not prove that the old worker has unwound.
            // Do not start another process while any consumer owns its endpoint.
            if Arc::strong_count(&self.adapters[index].endpoint) != 1
                || self
                    .reasoning_jobs
                    .values()
                    .any(|job| job.resources.contains(&adapter_id))
            {
                continue;
            }
            let claims = self.reasoning_claims(&HashSet::from([adapter_id.clone()]))?;
            match self.ensure_resources_available(&claims) {
                Ok(()) => {}
                Err(HostError::ResourceBusy { .. }) => continue,
                Err(error) => return Err(error),
            }
            let definition = self
                .reconnect_service()
                .definition(&definition_version)
                .map_err(error)?;
            let claim = self
                .reconnect_service()
                .claim(&definition_version, &state.version_id, now)
                .map_err(error)?;
            self.reconnect_states
                .insert(adapter_id.clone(), ConnectionState::Connecting);
            self.adapters[index].endpoint.abort();
            let config = self.adapters[index].config.clone();
            let cancelled = Arc::new(AtomicBool::new(false));
            let cancellation = cancelled.clone();
            let (send, response) = mpsc::sync_channel(1);
            let worker = std::thread::Builder::new()
                .name("arcs-reconnect".into())
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        StdioAdapter::handshake_cancellable(
                            &mut config.command(),
                            Duration::from_millis(definition.policy.handshake_timeout_ms),
                            cancellation,
                        )
                        .and_then(|mut endpoint| {
                            endpoint
                                .set_dialog_timeout(Duration::from_millis(config.timeout_ms))?;
                            Ok(endpoint)
                        })
                    }))
                    .map_err(|_| ConnectionFailure::Transport)
                    .and_then(|result| {
                        result.map_err(|error| match error {
                            StdioError::Timeout => ConnectionFailure::Timeout,
                            StdioError::UnsupportedVersion(_)
                            | StdioError::CorrelationMismatch
                            | StdioError::UnexpectedMessage => ConnectionFailure::Contract,
                            _ => ConnectionFailure::Transport,
                        })
                    });
                    let _ = send.send(result);
                });
            match worker {
                Ok(worker) => {
                    self.reconnect_jobs.insert(
                        adapter_id.clone(),
                        ReconnectJob {
                            claim,
                            cancelled,
                            response,
                            worker: Some(worker),
                            result: None,
                            installed: false,
                            claims,
                        },
                    );
                }
                Err(_) => {
                    let receipt = self
                        .reconnect_service()
                        .finish(
                            &definition_version,
                            &claim.version_id,
                            Some(ConnectionFailure::Transport),
                            now,
                        )
                        .map_err(error)?;
                    self.reconnect_states
                        .insert(adapter_id.clone(), receipt.record.state);
                }
            }
            self.reconnect_cursor = Some(adapter_id);
        }
        Ok(())
    }
    fn collect_reconnections(&mut self, now: u64) -> Result<(), HostError> {
        let mut ids = self.reconnect_jobs.keys().cloned().collect::<Vec<_>>();
        ids.sort();
        for adapter_id in ids {
            let mut job = self.reconnect_jobs.remove(&adapter_id).unwrap();
            if job.result.is_none() {
                job.result = match job.response.try_recv() {
                    Ok(result) => Some(result),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => Some(Err(ConnectionFailure::Transport)),
                };
            }
            if job.result.is_none() {
                self.reconnect_jobs.insert(adapter_id, job);
                continue;
            }
            let definition_version = job.claim.record.definition_version_id.clone();
            let definition = match self.reconnect_service().definition(&definition_version) {
                Ok(definition) => definition,
                Err(cause) => {
                    self.reconnect_jobs.insert(adapter_id, job);
                    return Err(error(cause));
                }
            };
            let failure = match job.result.as_ref().unwrap() {
                Err(failure) => Some(*failure),
                Ok(endpoint) if endpoint.manifest() != &definition.manifest => {
                    Some(ConnectionFailure::Contract)
                }
                Ok(_) => None,
            };
            let receipt = match self.reconnect_service().finish(
                &definition_version,
                &job.claim.version_id,
                failure,
                now,
            ) {
                Ok(receipt) => receipt,
                Err(cause) => {
                    self.reconnect_jobs.insert(adapter_id, job);
                    return Err(error(cause));
                }
            };
            self.reconnect_states
                .insert(adapter_id.clone(), receipt.record.state);
            if failure.is_none() {
                let Ok(endpoint) = job.result.take().unwrap() else {
                    unreachable!()
                };
                let adapter = self
                    .adapters
                    .iter_mut()
                    .find(|adapter| adapter.config.grant.adapter_id == adapter_id)
                    .unwrap();
                adapter.endpoint = Arc::new(endpoint);
                job.installed = true;
                self.unavailable_adapters.remove(&adapter_id);
            }
            if let Some(worker) = job.worker.take()
                && worker.is_finished()
            {
                let _ = worker.join();
            }
        }
        Ok(())
    }
    pub(super) fn cancel_reconnections(&mut self) -> Result<(), HostError> {
        self.reconnect_stopped = true;
        for job in self.reconnect_jobs.values() {
            job.cancelled.store(true, Ordering::Release);
        }
        let jobs = self.reconnect_jobs.drain().collect::<Vec<_>>();
        let mut first_error = None;
        for (adapter_id, mut job) in jobs {
            if let Some(worker) = job.worker.take() {
                let _ = worker.join();
            }
            let result = now().and_then(|now| {
                self.reconnect_service()
                    .finish(
                        &job.claim.record.definition_version_id,
                        &job.claim.version_id,
                        Some(ConnectionFailure::Interrupted),
                        now,
                    )
                    .map_err(error)
            });
            match result {
                Ok(receipt) => {
                    self.reconnect_states
                        .insert(adapter_id, receipt.record.state);
                }
                Err(error) if first_error.is_none() => first_error = Some(error),
                Err(_) => {}
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}
