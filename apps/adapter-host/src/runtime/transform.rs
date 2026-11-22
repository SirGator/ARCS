//! Only the adapter call runs on a worker. Core state stays on the Host.
use super::*;
use arcs::runtime::{InvocationService, InvocationStatus};
use arcs::transform::{
    PreparedTransform, TransformAdapter, TransformError, TransformPreparation, TransformResponse,
    transform_invocation_id,
};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::Instant;

pub(super) struct PendingTransform {
    prepared: PreparedTransform,
    response: Receiver<Result<TransformResponse, AdapterCallError>>,
    worker: JoinHandle<()>,
    deadline: Instant,
    timed_out: bool,
    standalone: bool,
    tasks: HashSet<(std::path::PathBuf, u64)>,
    pub(super) claims: resources::ResourceClaims,
}

impl PendingTransform {
    pub(super) fn adapter_id(&self) -> &AdapterId {
        &self.prepared.invocation().capability.adapter_id
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TransformWorkerReport {
    pub invocation_id: String,
    pub capability: CapabilityRef,
    pub input_version_id: VersionId,
    pub output_schema_id: SchemaId,
}

impl Host {
    pub fn transform_workers(&self) -> Vec<TransformWorkerReport> {
        let mut entries = self
            .transform_jobs
            .values()
            .map(|job| {
                let inv = job.prepared.invocation();
                TransformWorkerReport {
                    invocation_id: inv.invocation_id.clone(),
                    capability: inv.capability.clone(),
                    input_version_id: inv.input_version_id.clone(),
                    output_schema_id: inv.output_schema_id.clone(),
                }
            })
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| a.invocation_id.cmp(&b.invocation_id));
        entries
    }

    fn transform_service(&mut self) -> TransformService<'_> {
        TransformService::new(
            &self.registry,
            &self.schemas,
            &self.store,
            &mut self.ids,
            &SystemClock,
        )
    }

    /// Poll until completion or error. None means the call is queued or running.
    /// Persisted successes can replay even when this Host's adapter session is busy.
    pub fn poll_transform(
        &mut self,
        capability: &CapabilityRef,
        input: &VersionId,
        output_schema: &SchemaId,
        limit: usize,
    ) -> Result<Option<Artifact>, HostError> {
        self.poll_transform_for(capability, input, output_schema, limit, None)
    }

    pub(crate) fn poll_dependency_transform(
        &mut self,
        task: (&Path, u64),
        capability: &CapabilityRef,
        input: &VersionId,
        output_schema: &SchemaId,
        limit: usize,
    ) -> Result<Option<Artifact>, HostError> {
        self.poll_transform_for(capability, input, output_schema, limit, Some(task))
    }

    fn poll_transform_for(
        &mut self,
        capability: &CapabilityRef,
        input: &VersionId,
        output_schema: &SchemaId,
        limit: usize,
        task: Option<(&Path, u64)>,
    ) -> Result<Option<Artifact>, HostError> {
        if !(1..=32).contains(&limit) {
            return Err(invalid("transform worker limit must be 1..32"));
        }
        let (adapter, _) = self
            .registry
            .authorized_capability(&capability.adapter_id, &capability.capability_id)
            .map_err(HostError::Registration)?;
        let key = transform_invocation_id(
            capability,
            input,
            output_schema,
            &adapter.manifest().adapter_version,
        );
        if let Some(job) = self.transform_jobs.get_mut(&key) {
            let inv = job.prepared.invocation();
            if &inv.capability != capability
                || &inv.input_version_id != input
                || &inv.output_schema_id != output_schema
            {
                return Err(invalid("running transform request identity changed"));
            }
            if let Some((journal, sequence)) = task {
                job.tasks.insert((journal.to_path_buf(), sequence));
            } else {
                job.standalone = true;
            }
            return self.collect_transform(&key);
        }
        let replay = InvocationService::new(&self.store, &self.schemas, &SystemClock)
            .lookup(&key)
            .map_err(|source| transform_error(&capability.adapter_id, source.into()))?
            .is_some_and(|state| state.status == InvocationStatus::Succeeded);
        let claims = if replay {
            resources::ResourceClaims::new()
        } else {
            if self.transform_jobs.len() >= limit {
                return Ok(None);
            }
            match self.ensure_adapter_available(&capability.adapter_id) {
                Err(HostError::AdapterBusy(_) | HostError::ResourceBusy { .. }) => return Ok(None),
                other => other?,
            }
            let claims = self.reasoning_claims(&HashSet::from([capability.adapter_id.clone()]))?;
            match self.ensure_resources_available(&claims) {
                Err(HostError::ResourceBusy { .. }) => return Ok(None),
                other => other?,
            }
            claims
        };
        let adapter = self
            .adapters
            .iter()
            .find(|a| a.config.grant.adapter_id == capability.adapter_id)
            .ok_or_else(|| invalid("unknown transform adapter"))?;
        let endpoint = Arc::clone(&adapter.endpoint);
        let timeout = Duration::from_millis(adapter.config.timeout_ms);
        let prepared = match self
            .transform_service()
            .prepare_for_worker(endpoint.as_ref(), capability, input, output_schema)
            .map_err(|error| transform_error(&capability.adapter_id, error))?
        {
            TransformPreparation::Complete(artifact) => return Ok(Some(*artifact)),
            TransformPreparation::Dispatch(prepared) => *prepared,
        };
        let invocation = prepared.invocation().clone();
        let deadline = Instant::now() + timeout;
        let (send, response) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("arcs-transform".into())
            .spawn(move || {
                // Stdio exchange enforces the configured RPC timeout.
                let result = TransformAdapter::transform(endpoint.as_ref(), &invocation);
                let result = if Instant::now() >= deadline {
                    Err(AdapterCallError::Timeout)
                } else {
                    result
                };
                let _ = send.send(result);
            });
        match worker {
            Ok(worker) => {
                self.transform_jobs.insert(
                    key,
                    PendingTransform {
                        prepared,
                        response,
                        worker,
                        deadline,
                        timed_out: false,
                        standalone: task.is_none(),
                        tasks: task
                            .into_iter()
                            .map(|(journal, sequence)| (journal.to_path_buf(), sequence))
                            .collect(),
                        claims,
                    },
                );
                Ok(None)
            }
            Err(error) => self
                .transform_service()
                .finish(
                    prepared,
                    Err(AdapterCallError::Unavailable(format!(
                        "cannot start transform worker: {error}"
                    ))),
                )
                .map(Some)
                .map_err(|error| transform_error(&capability.adapter_id, error)),
        }
    }

    pub(crate) fn dependency_transform_tasks(&self, journal: &Path) -> HashSet<u64> {
        self.transform_jobs
            .values()
            .flat_map(|job| &job.tasks)
            .filter_map(|(path, sequence)| (path == journal).then_some(*sequence))
            .collect()
    }

    pub(crate) fn release_dependency_transforms(
        &mut self,
        journal: &Path,
        abandoned: &HashSet<u64>,
    ) {
        let mut orphaned = Vec::new();
        for (key, job) in &mut self.transform_jobs {
            job.tasks
                .retain(|(path, sequence)| path != journal || !abandoned.contains(sequence));
            if job.tasks.is_empty() && !job.standalone {
                orphaned.push(key.clone());
            }
        }
        for key in orphaned {
            // Transform is pure. Don't attach its result to a stopped task.
            // Keep its session busy until the adapter call has ended.
            self.take_ready_transform(&key);
        }
    }

    fn collect_transform(&mut self, key: &str) -> Result<Option<Artifact>, HostError> {
        let Some((prepared, response)) = self.take_ready_transform(key) else {
            return Ok(None);
        };
        let adapter_id = prepared.invocation().capability.adapter_id.clone();
        self.transform_service()
            .finish(prepared, response)
            .map(Some)
            .map_err(|error| transform_error(&adapter_id, error))
    }

    fn take_ready_transform(
        &mut self,
        key: &str,
    ) -> Option<(
        PreparedTransform,
        Result<TransformResponse, AdapterCallError>,
    )> {
        let job = &self.transform_jobs[key];
        if !job.worker.is_finished() {
            if !job.timed_out && Instant::now() >= job.deadline {
                let adapter_id = job.adapter_id().clone();
                self.abort_transform_adapter(&adapter_id);
                self.transform_jobs.get_mut(key).unwrap().timed_out = true;
            }
            return None;
        }
        let response = if job.timed_out {
            Err(AdapterCallError::Timeout)
        } else {
            job.response.try_recv().unwrap_or_else(|_| {
                Err(AdapterCallError::Unavailable(
                    "transform worker disconnected".into(),
                ))
            })
        };
        let job = self
            .transform_jobs
            .remove(key)
            .expect("checked transform worker");
        let adapter_id = job.adapter_id().clone();
        let _ = job.worker.join();
        if !job.timed_out
            && matches!(
                response,
                Err(AdapterCallError::Unavailable(_) | AdapterCallError::Timeout)
            )
        {
            self.abort_transform_adapter(&adapter_id);
        }
        Some((job.prepared, response))
    }

    fn abort_transform_adapter(&mut self, adapter_id: &AdapterId) {
        if let Some(adapter) = self
            .adapters
            .iter()
            .find(|a| &a.config.grant.adapter_id == adapter_id)
        {
            adapter.endpoint.abort();
        }
        self.unavailable_adapters.insert(adapter_id.clone());
    }

    pub(super) fn cancel_transform_jobs(&mut self) -> HashSet<AdapterId> {
        let adapters = self
            .transform_jobs
            .values()
            .map(|job| job.adapter_id().clone())
            .collect::<HashSet<_>>();
        for id in &adapters {
            self.abort_transform_adapter(id);
        }
        for (_, job) in self.transform_jobs.drain() {
            let _ = job.worker.join();
        }
        adapters
    }
}

fn transform_error(adapter: &AdapterId, source: TransformError) -> HostError {
    HostError::Transform {
        adapter: adapter.clone(),
        source,
    }
}
